use std::collections::HashMap;

use topaz_diag::{
    Code, Diagnostic, FileId, Label, SourceMap, Span, has_errors, render, render_json,
};
use topaz_resolve::{ResolveOutput, ResolvedReferenceFact};
use topaz_syntax::{LangVersion, TokenKind, ast};

const PROFILE_DISALLOWED: Code = Code::new("TPZ5801");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckProfile {
    AgentPack,
    TestProfile,
    Bootstrap,
}

impl CheckProfile {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "agent-pack" => Some(Self::AgentPack),
            "test-profile" => Some(Self::TestProfile),
            "bootstrap" => Some(Self::Bootstrap),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AgentPack => "agent-pack",
            Self::TestProfile => "test-profile",
            Self::Bootstrap => "bootstrap",
        }
    }

    fn composition_rule(self) -> Option<&'static str> {
        match self {
            Self::AgentPack => Some("agent-pack/no-composition"),
            Self::TestProfile => Some("test-profile/no-composition"),
            Self::Bootstrap => None,
        }
    }

    fn test_framework_rule(self) -> &'static str {
        match self {
            Self::AgentPack => "agent-pack/no-test-framework",
            Self::TestProfile => "test-profile/no-test-framework",
            Self::Bootstrap => "bootstrap/no-test-framework",
        }
    }

    fn is_bootstrap(self) -> bool {
        matches!(self, Self::Bootstrap)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProfileDiagnostic {
    pub(crate) diagnostic: Diagnostic,
    pub(crate) rule: Option<&'static str>,
}

impl ProfileDiagnostic {
    pub(crate) fn compiler(diagnostic: Diagnostic) -> Self {
        Self {
            diagnostic,
            rule: None,
        }
    }

    pub(crate) fn policy(rule: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self {
            diagnostic: Diagnostic::error(
                PROFILE_DISALLOWED,
                message.into(),
                Label::new(span, "Bootstrap Profile package boundary"),
            )
            .with_note(format!("profile rule: {rule}")),
            rule: Some(rule),
        }
    }
}

pub(crate) fn collect_typed(
    profile: CheckProfile,
    typed: Option<&topaz_hir::TypedUnit>,
) -> Vec<ProfileDiagnostic> {
    if !profile.is_bootstrap() {
        return Vec::new();
    }
    let Some(typed) = typed else {
        return Vec::new();
    };
    let mut findings = typed
        .nodes
        .iter()
        .filter(|node| node.ambient)
        .map(|node| {
            ProfileDiagnostic::policy(
                "bootstrap/no-ambient-type",
                "the compiler kernel may not retain an ambient or unexplained type",
                node.span,
            )
        })
        .collect::<Vec<_>>();
    findings.extend(
        typed
            .calls
            .iter()
            .filter(|call| call.callee_type.has_hole() || call.result_type.has_hole())
            .map(|call| {
                ProfileDiagnostic::policy(
                    "bootstrap/no-ambient-type",
                    "the compiler kernel may not retain an unexplained callable or result type",
                    call.span,
                )
            }),
    );
    for node in &typed.nodes {
        if let Some((rule, message)) = bootstrap_type_denial(&node.ty) {
            findings.push(ProfileDiagnostic::policy(rule, message, node.span));
        }
    }
    for call in &typed.calls {
        for ty in [&call.callee_type, &call.result_type] {
            if let Some((rule, message)) = bootstrap_type_denial(ty) {
                findings.push(ProfileDiagnostic::policy(rule, message, call.span));
                break;
            }
        }
    }
    for capture in &typed.captures {
        if let Some((rule, message)) = bootstrap_type_denial(&capture.ty) {
            findings.push(ProfileDiagnostic::policy(
                rule,
                message,
                capture.reference_span,
            ));
        }
    }
    findings.sort_by_key(|finding| {
        let span = finding.diagnostic.primary.span;
        (span.file.0, span.lo, span.hi)
    });
    findings.dedup_by_key(|finding| finding.diagnostic.primary.span);
    findings
}

fn bootstrap_type_denial(ty: &topaz_hir::SemanticType) -> Option<(&'static str, &'static str)> {
    use topaz_hir::SemanticType as T;
    match ty {
        T::Primitive(topaz_hir::SemanticPrimitive::Float)
        | T::Literal(topaz_hir::SemanticLiteral::Float(_)) => Some((
            "bootstrap/no-float",
            "`float` is not available to the deterministic Bootstrap Profile",
        )),
        T::File | T::Path => Some((
            "bootstrap/no-resource",
            "host file and path values are not available to the Bootstrap Profile",
        )),
        T::Regex
        | T::Match
        | T::TomlValue
        | T::Url
        | T::Date
        | T::BigInt
        | T::Decimal
        | T::RoundingMode => Some((
            "bootstrap/no-host-leaf",
            "this runtime value family is outside the Bootstrap Profile inventory",
        )),
        T::Union(values) => values.iter().find_map(bootstrap_type_denial),
        T::Record(fields) => fields
            .iter()
            .find_map(|field| bootstrap_type_denial(&field.ty)),
        T::Constructor { arguments, .. }
        | T::Foreign { arguments, .. }
        | T::Enum { arguments, .. }
        | T::NominalRecord { arguments, .. }
        | T::Newtype { arguments, .. } => arguments.iter().find_map(bootstrap_type_denial),
        T::Function {
            parameters,
            variadic,
            result,
        } => parameters
            .iter()
            .find_map(bootstrap_type_denial)
            .or_else(|| variadic.as_deref().and_then(bootstrap_type_denial))
            .or_else(|| bootstrap_type_denial(result)),
        T::Primitive(_)
        | T::Literal(_)
        | T::Rigid { .. }
        | T::Template
        | T::JsonValue
        | T::Bytes
        | T::ByteBuffer
        | T::Unknown
        | T::InferenceVariable => None,
    }
}

pub(crate) fn collect(out: &ResolveOutput, profile: CheckProfile) -> Vec<ProfileDiagnostic> {
    let mut findings = Vec::new();
    let references = reference_index(&out.name_facts.references);
    for module in &out.modules {
        // Virtual stdlib sources are compiler-owned implementations, not authored
        // profile input. User imports and calls are still checked at their own AST
        // sites; skipping the implementation prevents one forbidden `std.test`
        // import from expanding into a diagnostic for every internal `Test.*` call.
        if module.identity == "std" || module.identity.starts_with("std.") {
            continue;
        }
        let src = out.map.file(module.file).src();
        collect_program(
            profile,
            module.file,
            &module.program,
            src,
            &references,
            &mut findings,
        );
    }
    findings.sort_by_key(|finding| {
        let span = finding.diagnostic.primary.span;
        (span.file.0, span.lo, span.hi, finding.rule)
    });
    findings
}

type ReferenceIndex<'a> = HashMap<Span, &'a ResolvedReferenceFact>;

fn reference_index(references: &[ResolvedReferenceFact]) -> ReferenceIndex<'_> {
    let mut index = HashMap::with_capacity(references.len());
    for reference in references {
        index.entry(reference.span).or_insert(reference);
    }
    index
}

fn collect_program(
    profile: CheckProfile,
    file: FileId,
    program: &ast::Program,
    src: &str,
    references: &ReferenceIndex<'_>,
    findings: &mut Vec<ProfileDiagnostic>,
) {
    let compose_tokens: Vec<Span> = topaz_lexer::lex(file, src)
        .tokens
        .into_iter()
        .filter(|token| matches!(token.kind, TokenKind::GtGt))
        .map(|token| token.span)
        .collect();
    let mut collector = Collector {
        profile,
        src,
        references,
        compose_tokens: &compose_tokens,
        findings,
    };
    for stmt in &program.items {
        collector.stmt(stmt);
    }
}

struct Collector<'a> {
    profile: CheckProfile,
    src: &'a str,
    references: &'a ReferenceIndex<'a>,
    compose_tokens: &'a [Span],
    findings: &'a mut Vec<ProfileDiagnostic>,
}

impl Collector<'_> {
    fn bootstrap_finding(&mut self, span: Span, rule: &'static str, message: impl Into<String>) {
        self.findings.push(ProfileDiagnostic {
            diagnostic: Diagnostic::error(
                PROFILE_DISALLOWED,
                message.into(),
                Label::new(span, "not in the deterministic compiler-kernel inventory"),
            )
            .with_note(format!("profile rule: {rule}"))
            .with_note(
                "move this effect to the Rust host shell or use a deterministic admitted value/library leaf",
            ),
            rule: Some(rule),
        });
    }

    fn reference(&self, span: Span) -> Option<&topaz_resolve::ResolvedReferenceFact> {
        self.references.get(&span).copied()
    }

    fn stmt(&mut self, stmt: &ast::Stmt) {
        match &stmt.kind {
            ast::StmtKind::Import(item) => {
                if self.profile.is_bootstrap() {
                    let target = item
                        .path
                        .segments
                        .iter()
                        .map(|segment| text(self.src, segment.span))
                        .collect::<Vec<_>>()
                        .join(".");
                    if bootstrap_denied_module(&target) {
                        self.bootstrap_finding(
                            item.path.span,
                            "bootstrap/no-host-module",
                            format!("module `{target}` is not available to the Bootstrap Profile"),
                        );
                    }
                }
                if item.path.segments.len() == 2
                    && text(self.src, item.path.segments[0].span) == "std"
                    && text(self.src, item.path.segments[1].span) == "test"
                {
                    self.test_framework_finding(item.path.span, "test-only `std.test` import");
                }
            }
            ast::StmtKind::TypeAlias(_)
            | ast::StmtKind::Enum(_)
            | ast::StmtKind::Newtype(_)
            | ast::StmtKind::Continue { .. } => {}
            ast::StmtKind::Export(inner) => self.stmt(inner),
            ast::StmtKind::Function(decl) => self.function(decl),
            ast::StmtKind::Record(decl) => {
                for field in &decl.fields {
                    if let Some(default) = &field.default {
                        self.expr(default);
                    }
                }
            }
            ast::StmtKind::Impl(decl) => {
                for method in &decl.methods {
                    self.function(&method.decl);
                }
            }
            ast::StmtKind::Protocol(decl) => {
                for method in &decl.methods {
                    self.function(method);
                }
            }
            ast::StmtKind::Let { pattern, value, .. } => {
                self.pattern(pattern);
                self.expr(value);
            }
            ast::StmtKind::Const { value, .. } => self.expr(value),
            ast::StmtKind::Defer(value) => self.expr(value),
            ast::StmtKind::Assign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            ast::StmtKind::Return(value) | ast::StmtKind::Break { value, .. } => {
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            ast::StmtKind::Using { value, body, .. } => {
                if self.profile.is_bootstrap() {
                    self.bootstrap_finding(
                        stmt.span,
                        "bootstrap/no-resource",
                        "`using` and host resource values are not available to the Bootstrap Profile",
                    );
                }
                self.expr(value);
                self.block(body);
            }
            ast::StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            ast::StmtKind::Expr(expr) => self.expr(expr),
        }
    }

    fn function(&mut self, decl: &ast::FunctionDecl) {
        for param in &decl.params {
            if let Some(default) = &param.default {
                self.expr(default);
            }
        }
        self.block(&decl.body);
    }

    fn block(&mut self, block: &ast::Block) {
        for stmt in &block.stmts {
            self.stmt(stmt);
        }
        if let Some(tail) = &block.tail {
            self.expr(tail);
        }
    }

    fn pattern(&mut self, pattern: &ast::Pattern) {
        match &pattern.kind {
            ast::PatternKind::Or(patterns)
            | ast::PatternKind::Constructor { args: patterns, .. } => {
                for pattern in patterns {
                    self.pattern(pattern);
                }
            }
            ast::PatternKind::Literal(expr) => self.expr(expr),
            ast::PatternKind::Range { lo, hi, .. } => {
                self.expr(lo);
                self.expr(hi);
            }
            ast::PatternKind::List(elements) => {
                for element in elements {
                    match element {
                        ast::ListPatternElem::Pattern(pattern)
                        | ast::ListPatternElem::Rest(Some(pattern)) => self.pattern(pattern),
                        ast::ListPatternElem::Rest(None) => {}
                    }
                }
            }
            ast::PatternKind::Record(fields) | ast::PatternKind::NominalRecord { fields, .. } => {
                for field in fields {
                    if let Some(pattern) = &field.pattern {
                        self.pattern(pattern);
                    }
                }
            }
            ast::PatternKind::Wildcard
            | ast::PatternKind::Binding(_)
            | ast::PatternKind::Typed { .. } => {}
        }
    }

    fn expr(&mut self, expr: &ast::Expr) {
        match &expr.kind {
            ast::ExprKind::Int
            | ast::ExprKind::Bool(_)
            | ast::ExprKind::Null
            | ast::ExprKind::Unit
            | ast::ExprKind::Placeholder => {}
            ast::ExprKind::Float => {
                if self.profile.is_bootstrap() {
                    self.bootstrap_finding(
                        expr.span,
                        "bootstrap/no-float",
                        "`float` is not available to the deterministic Bootstrap Profile",
                    );
                }
            }
            ast::ExprKind::Duration(_) => {
                if self.profile.is_bootstrap() {
                    self.bootstrap_finding(
                        expr.span,
                        "bootstrap/no-duration",
                        "duration values are not available to the deterministic Bootstrap Profile",
                    );
                }
            }
            ast::ExprKind::Ident => {
                let name = text(self.src, expr.span);
                if self.profile.is_bootstrap()
                    && bootstrap_denied_builtin(name)
                    && self
                        .reference(expr.span)
                        .is_none_or(|reference| reference.target_span.is_none())
                {
                    self.bootstrap_finding(
                        expr.span,
                        "bootstrap/no-host-leaf",
                        format!("runtime leaf `{name}` is not available to the Bootstrap Profile"),
                    );
                } else if matches!(self.profile, CheckProfile::AgentPack)
                    && name == "assert"
                    && self
                        .reference(expr.span)
                        .is_none_or(|reference| reference.target_span.is_none())
                {
                    self.findings.push(ProfileDiagnostic {
                        diagnostic: Diagnostic::error(
                            PROFILE_DISALLOWED,
                            "`assert` is not allowed by profile `agent-pack`",
                            Label::new(expr.span, "test-only function reference"),
                        )
                        .with_note("profile rule: agent-pack/no-assert")
                        .with_note(
                            "use `--profile test-profile` only for test code, or express the failure with canonical Result/control flow",
                        ),
                        rule: Some("agent-pack/no-assert"),
                    });
                }
            }
            ast::ExprKind::String(lit) => {
                for part in &lit.parts {
                    if let ast::StringPart::Interpolation(expr) = part {
                        self.expr(expr);
                    }
                }
            }
            ast::ExprKind::Paren(expr)
            | ast::ExprKind::Try(expr)
            | ast::ExprKind::Unary { operand: expr, .. } => self.expr(expr),
            ast::ExprKind::Block(block) => self.block(block),
            ast::ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.expr(cond);
                self.block(then_block);
                if let Some(else_branch) = else_branch {
                    self.expr(else_branch);
                }
            }
            ast::ExprKind::Match { scrutinee, cases } => {
                self.expr(scrutinee);
                for case in cases {
                    self.pattern(&case.pattern);
                    if let Some(guard) = &case.guard {
                        self.expr(guard);
                    }
                    match &case.body {
                        ast::CaseArmBody::Expr(expr) => self.expr(expr),
                        ast::CaseArmBody::Return { value, .. } => {
                            if let Some(value) = value {
                                self.expr(value);
                            }
                        }
                    }
                }
            }
            ast::ExprKind::For {
                pattern,
                iter,
                body,
            } => {
                self.pattern(pattern);
                self.expr(iter);
                self.block(body);
            }
            ast::ExprKind::Loop { body, .. } => self.block(body),
            ast::ExprKind::Concurrent {
                timeout,
                arms,
                else_block,
            } => {
                if self.profile.is_bootstrap() {
                    self.bootstrap_finding(
                        expr.span,
                        "bootstrap/no-concurrency",
                        "`concurrent` is not available to the deterministic Bootstrap Profile",
                    );
                }
                if let Some(timeout) = timeout {
                    self.expr(timeout);
                }
                for arm in arms {
                    self.expr(&arm.value);
                }
                if let Some(else_block) = else_block {
                    self.block(else_block);
                }
            }
            ast::ExprKind::Call { callee, args, .. } => {
                self.expr(callee);
                for arg in args {
                    match arg {
                        ast::CallArg::Positional(expr)
                        | ast::CallArg::Spread(expr)
                        | ast::CallArg::Named { value: expr, .. } => self.expr(expr),
                    }
                }
            }
            ast::ExprKind::Member { object, field }
            | ast::ExprKind::OptionalAccess { object, field } => {
                if let ast::ExprKind::Ident = object.kind {
                    let resolved_identity = self.reference(field.span).and_then(|reference| {
                        reference.target_module.as_ref().map(|module| {
                            format!(
                                "{module}::{}",
                                reference
                                    .target_name
                                    .as_deref()
                                    .unwrap_or_else(|| text(self.src, field.span))
                            )
                        })
                    });
                    let object_is_resolved = self
                        .reference(object.span)
                        .is_some_and(|reference| reference.target_span.is_some());
                    let profile_identity = resolved_identity.clone().or_else(|| {
                        (!object_is_resolved).then(|| {
                            format!(
                                "{}::{}",
                                text(self.src, object.span),
                                text(self.src, field.span)
                            )
                        })
                    });
                    if self.profile.is_bootstrap()
                        && let Some(identity) = profile_identity.as_deref()
                        && bootstrap_denied_leaf(identity)
                    {
                        self.bootstrap_finding(
                            expr.span,
                            "bootstrap/no-host-leaf",
                            format!(
                                "runtime leaf `{identity}` is not available to the Bootstrap Profile"
                            ),
                        );
                    }
                    let test_namespace = resolved_identity.as_deref().map_or_else(
                        || !object_is_resolved && text(self.src, object.span) == "Test",
                        test_framework_identity,
                    );
                    if test_namespace && is_test_framework_member(text(self.src, field.span)) {
                        self.test_framework_finding(
                            expr.span,
                            "test-only `Test.*` namespace member",
                        );
                    }
                }
                self.expr(object)
            }
            ast::ExprKind::Index { object, index } => {
                self.expr(object);
                self.expr(index);
            }
            ast::ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ast::ExprKind::Compose { lhs, rhs } => {
                let span = self
                    .compose_tokens
                    .iter()
                    .copied()
                    .find(|span| span.lo >= lhs.span.hi && span.hi <= rhs.span.lo)
                    .unwrap_or(Span::new(expr.span.file, lhs.span.hi, rhs.span.lo));
                if let Some(rule) = self.profile.composition_rule() {
                    self.findings.push(ProfileDiagnostic {
                        diagnostic: Diagnostic::error(
                            PROFILE_DISALLOWED,
                            format!(
                                "function composition is not allowed by profile `{}`",
                                self.profile.as_str()
                            ),
                            Label::new(span, "profile-disallowed `>>`"),
                        )
                        .with_note(format!("profile rule: {rule}"))
                        .with_note(
                            "use an explicit lambda, or a pipeline only when it preserves the intended semantics",
                        ),
                        rule: Some(rule),
                    });
                }
                self.expr(lhs);
                self.expr(rhs);
            }
            ast::ExprKind::Range { lo, hi, step, .. } => {
                self.expr(lo);
                self.expr(hi);
                if let Some(step) = step {
                    self.expr(step);
                }
            }
            ast::ExprKind::Pipe { lhs, rhs } => {
                self.expr(lhs);
                if let ast::PipeRhs::Expr(rhs) = rhs.as_ref() {
                    self.expr(rhs);
                }
            }
            ast::ExprKind::Lambda { body, .. } => self.expr(body),
            ast::ExprKind::RecordLiteral { fields } => {
                for field in fields {
                    self.expr(&field.value);
                }
            }
            ast::ExprKind::RecordUpdate {
                base,
                spread,
                fields,
            } => {
                self.expr(base);
                if let Some(spread) = spread {
                    self.expr(spread);
                }
                for field in fields {
                    self.expr(&field.value);
                }
            }
            ast::ExprKind::Array(elements) => {
                for element in elements {
                    match element {
                        ast::ArrayElement::Expr(expr) | ast::ArrayElement::Spread(expr) => {
                            self.expr(expr)
                        }
                    }
                }
            }
            ast::ExprKind::SetLiteral(elements) => {
                for element in elements {
                    self.expr(element);
                }
            }
            ast::ExprKind::MapLiteral(entries) => {
                for (key, value) in entries {
                    self.expr(key);
                    self.expr(value);
                }
            }
            ast::ExprKind::Comprehension { clauses, body, .. } => {
                for clause in clauses {
                    match clause {
                        ast::CompClause::For { pattern, iter } => {
                            self.pattern(pattern);
                            self.expr(iter);
                        }
                        ast::CompClause::If(cond) => self.expr(cond),
                    }
                }
                match body.as_ref() {
                    ast::CompBody::Elem(expr) => self.expr(expr),
                    ast::CompBody::Entry { key, value } => {
                        self.expr(key);
                        self.expr(value);
                    }
                }
            }
        }
    }

    fn test_framework_finding(&mut self, span: Span, label: &'static str) {
        let rule = self.profile.test_framework_rule();
        self.findings.push(ProfileDiagnostic {
            diagnostic: Diagnostic::error(
                PROFILE_DISALLOWED,
                format!(
                    "test-framework APIs are not allowed by profile `{}`",
                    self.profile.as_str()
                ),
                Label::new(span, label),
            )
            .with_note(format!("profile rule: {rule}"))
            .with_note(
                "the executable test-profile grants only the canonical free `assert(...)` function",
            ),
            rule: Some(rule),
        });
    }
}

pub(crate) fn render_profile_diagnostic(
    profile: CheckProfile,
    finding: &ProfileDiagnostic,
    map: &SourceMap,
    json: bool,
) -> String {
    if !json {
        return format!(
            "profile[{}]\n{}",
            profile.as_str(),
            render(&finding.diagnostic, map)
        );
    }

    let mut out = String::from("{\"schema\":\"topaz.profile-diagnostic/v1\",\"profile\":");
    crate::push_json_string(&mut out, profile.as_str());
    out.push_str(",\"rule\":");
    match finding.rule {
        Some(rule) => crate::push_json_string(&mut out, rule),
        None => out.push_str("null"),
    }
    out.push_str(",\"diagnostic\":");
    out.push_str(&render_json(&finding.diagnostic, map));
    out.push_str(",\"fix\":");
    if let Some(replacement) = crate::lsp_diagnostic_replacement(&finding.diagnostic) {
        let span = finding.diagnostic.primary.span;
        let file = map.file(span.file);
        let start = file.line_col(span.lo);
        let end = file.line_col(span.hi);
        out.push_str("{\"applicability\":\"machine-applicable\",\"description\":");
        crate::push_json_string(&mut out, &format!("Replace with `{replacement}`"));
        out.push_str(",\"edit\":{\"file\":");
        crate::push_json_string(&mut out, file.name());
        out.push_str(&format!(
            ",\"line\":{},\"col\":{},\"endLine\":{},\"endCol\":{},\"lo\":{},\"hi\":{},\"replacement\":",
            start.line, start.col, end.line, end.col, span.lo, span.hi
        ));
        crate::push_json_string(&mut out, &replacement);
        out.push_str("}}");
    } else {
        out.push_str("null");
    }
    out.push('}');
    out
}

pub(crate) fn render_summary(
    profile: CheckProfile,
    version: LangVersion,
    diagnostics: &[ProfileDiagnostic],
) -> String {
    let error_count = diagnostics
        .iter()
        .filter(|finding| has_errors(std::slice::from_ref(&finding.diagnostic)))
        .count();
    format!(
        "{{\"schema\":\"topaz.profile-check/v1\",\"profile\":\"{}\",\"language\":\"topaz-{}\",\"status\":\"{}\",\"diagnosticCount\":{},\"errorCount\":{}}}",
        profile.as_str(),
        version.as_str(),
        if error_count == 0 { "pass" } else { "fail" },
        diagnostics.len(),
        error_count
    )
}

fn text(src: &str, span: Span) -> &str {
    &src[span.lo as usize..span.hi as usize]
}

fn is_test_framework_member(name: &str) -> bool {
    matches!(
        name,
        "assert"
            | "assertEq"
            | "assertNe"
            | "assertContains"
            | "assertOk"
            | "assertErr"
            | "assertSome"
            | "assertNone"
            | "assertGolden"
    )
}

fn identity_has_head(identity: &str, head: &str) -> bool {
    identity == head
        || identity
            .strip_prefix(head)
            .is_some_and(|tail| tail.starts_with('.') || tail.starts_with("::"))
}

fn test_framework_identity(identity: &str) -> bool {
    identity_has_head(identity, "std.test") || identity_has_head(identity, "Test")
}

fn bootstrap_denied_module(identity: &str) -> bool {
    [
        "std.fs",
        "std.io",
        "std.path",
        "std.cli",
        "std.http",
        "std.dom",
        "std.test",
        "std.regex",
        "std.codec",
        "std.crypto",
        "std.time",
        "std.random",
        "std.process",
        "std.database",
    ]
    .into_iter()
    .any(|head| identity_has_head(identity, head))
}

fn bootstrap_denied_builtin(name: &str) -> bool {
    matches!(name, "print" | "input" | "open" | "assert")
}

fn bootstrap_denied_leaf(identity: &str) -> bool {
    bootstrap_denied_module(identity)
        || [
            "FS",
            "Cli",
            "Path",
            "HTTP",
            "DOM",
            "Test",
            "Regex",
            "Date",
            "Math",
            "HMAC",
            "Base64",
            "Deflate",
            "Zlib",
            "ReedSolomon",
        ]
        .into_iter()
        .any(|head| identity_has_head(identity, head))
}

#[cfg(test)]
mod tests {
    use super::*;
    use topaz_parser::{ParseOptions, parse_with_options};
    use topaz_syntax::LangVersion;

    fn resolved_findings(src: &str, profile: CheckProfile) -> Vec<ProfileDiagnostic> {
        let mut provider = topaz_resolve::InMemoryProvider::new();
        provider.add_file("main.tpz", src);
        let out =
            topaz_resolve::resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
        assert!(!has_errors(&out.diagnostics), "{:#?}", out.diagnostics);
        collect(&out, profile)
    }

    fn findings(src: &str, profile: CheckProfile) -> Vec<ProfileDiagnostic> {
        let mut map = SourceMap::new();
        let file = map.add_file("main.tpz", src).expect("source");
        let parsed = parse_with_options(
            file,
            src,
            ParseOptions {
                language_version: LangVersion::CURRENT,
            },
        );
        assert!(
            !has_errors(&parsed.diagnostics),
            "{:#?}",
            parsed.diagnostics
        );
        let mut findings = Vec::new();
        let references = ReferenceIndex::new();
        collect_program(
            profile,
            file,
            &parsed.program,
            src,
            &references,
            &mut findings,
        );
        findings
    }

    #[test]
    fn summary_renders_the_selected_language_version() {
        let summary = render_summary(CheckProfile::AgentPack, LangVersion::V5_7, &[]);
        assert!(summary.contains("\"language\":\"topaz-5.7\""), "{summary}");
        assert!(!summary.contains("topaz-5.6"), "{summary}");
    }

    #[test]
    fn nested_type_closers_are_not_composition() {
        assert!(
            findings(
                "let value: Option<Result<int, string>> = None\n",
                CheckProfile::AgentPack,
            )
            .is_empty()
        );
    }

    #[test]
    fn composition_has_exact_operator_span_and_rule() {
        let src = "let combined = left >> right\n";
        let agent_findings = findings(src, CheckProfile::AgentPack);
        assert_eq!(agent_findings.len(), 1);
        assert_eq!(agent_findings[0].rule, Some("agent-pack/no-composition"));
        assert_eq!(text(src, agent_findings[0].diagnostic.primary.span), ">>");

        let test_findings = findings(src, CheckProfile::TestProfile);
        assert_eq!(test_findings.len(), 1);
        assert_eq!(test_findings[0].rule, Some("test-profile/no-composition"));
    }

    #[test]
    fn assert_override_is_explicit() {
        let src = "assert(true, \"ok\")\n";
        let agent = findings(src, CheckProfile::AgentPack);
        assert_eq!(agent.len(), 1);
        assert_eq!(agent[0].rule, Some("agent-pack/no-assert"));
        assert!(findings(src, CheckProfile::TestProfile).is_empty());
    }

    #[test]
    fn first_class_assert_reference_cannot_bypass_agent_pack() {
        let src = "let check = assert\ncheck(true, \"ok\")\n";
        let agent = findings(src, CheckProfile::AgentPack);
        assert_eq!(agent.len(), 1);
        assert_eq!(agent[0].rule, Some("agent-pack/no-assert"));
        assert!(findings(src, CheckProfile::TestProfile).is_empty());
    }

    #[test]
    fn test_namespace_and_std_test_stay_outside_both_executable_profiles() {
        for profile in [CheckProfile::AgentPack, CheckProfile::TestProfile] {
            let namespace = findings("Test.assert(true, \"ok\")\n", profile);
            assert_eq!(namespace.len(), 1);
            assert!(
                namespace[0]
                    .rule
                    .expect("rule")
                    .ends_with("/no-test-framework")
            );

            let import = findings(
                "import std.test { assert as check }\ncheck(true, \"ok\")\n",
                profile,
            );
            assert_eq!(import.len(), 1);
            assert!(
                import[0]
                    .rule
                    .expect("rule")
                    .ends_with("/no-test-framework")
            );
        }
    }

    #[test]
    fn user_nominal_named_test_is_not_the_test_namespace() {
        let src = "record Test { value: int }\nlet item = Test { value: 1 }\n";
        assert!(findings(src, CheckProfile::AgentPack).is_empty());
        assert!(findings(src, CheckProfile::TestProfile).is_empty());
    }

    #[test]
    fn resolved_local_assert_test_and_fs_values_are_not_builtin_names() {
        let assert_value = resolved_findings(
            "function assert(value: bool) -> bool { value }\nassert(true)\n",
            CheckProfile::AgentPack,
        );
        assert!(assert_value.is_empty(), "{assert_value:#?}");

        let test_value = resolved_findings(
            "let Test = { assert: (value: bool) => () }\nTest.assert(true)\n",
            CheckProfile::AgentPack,
        );
        assert!(test_value.is_empty(), "{test_value:#?}");

        let fs_value = resolved_findings(
            "let FS = { readText: (path: string) => path }\nFS.readText(\"ok\")\n",
            CheckProfile::Bootstrap,
        );
        assert!(fs_value.is_empty(), "{fs_value:#?}");
    }

    #[test]
    fn assert_text_and_member_call_are_not_free_assert_calls() {
        let src = "let note = \"assert(true)\"\nchecks.assert(true)\n";
        assert!(findings(src, CheckProfile::AgentPack).is_empty());
    }

    #[test]
    fn json_envelope_carries_profile_rule_and_safe_fix() {
        let mut map = SourceMap::new();
        let file = map.add_file("main.tpz", "lenght\n").expect("source");
        let finding = ProfileDiagnostic::compiler(Diagnostic::error(
            Code::new("TPZ5002"),
            "unknown name; did you mean `length`?",
            Label::new(Span::new(file, 0, 6), "not found"),
        ));
        let rendered = render_profile_diagnostic(CheckProfile::AgentPack, &finding, &map, true);
        assert!(rendered.contains("\"profile\":\"agent-pack\""));
        assert!(rendered.contains("\"rule\":null"));
        assert!(rendered.contains("\"replacement\":\"length\""));
        assert!(rendered.contains("\"lo\":0,\"hi\":6"));
    }

    #[test]
    fn bootstrap_denies_effects_but_not_same_spelled_local_values() {
        let denied = resolved_findings(
            "let action = print\nlet later = 1.5\nconcurrent { work: 1 }\n",
            CheckProfile::Bootstrap,
        );
        let rules = denied
            .iter()
            .filter_map(|finding| finding.rule)
            .collect::<Vec<_>>();
        assert!(rules.contains(&"bootstrap/no-host-leaf"));
        assert!(rules.contains(&"bootstrap/no-float"));
        assert!(rules.contains(&"bootstrap/no-concurrency"));

        let local = resolved_findings(
            "function print(value: int) -> int { value }\nlet result = print(1)\n",
            CheckProfile::Bootstrap,
        );
        assert!(local.is_empty(), "{:#?}", local);

        let aliased = resolved_findings(
            "import std.fs { readText as load }\nlet action = load\n",
            CheckProfile::Bootstrap,
        );
        assert!(
            aliased
                .iter()
                .any(|finding| finding.rule == Some("bootstrap/no-host-module")),
            "{aliased:#?}"
        );

        let path_module = findings(
            "import std.path { join }\nlet value = join\n",
            CheckProfile::Bootstrap,
        );
        assert!(
            path_module
                .iter()
                .any(|finding| finding.rule == Some("bootstrap/no-host-module")),
            "{path_module:#?}"
        );
    }

    #[test]
    fn bootstrap_denies_float_and_host_types_without_needing_a_literal() {
        let mut typed = topaz_hir::TypedUnit::new();
        typed.push_node(topaz_hir::TypedNode {
            module: "main".to_string(),
            kind: topaz_hir::TypedNodeKind::Declaration,
            span: Span::new(FileId(0), 0, 8),
            ty: topaz_hir::SemanticType::Function {
                parameters: vec![topaz_hir::SemanticType::Primitive(
                    topaz_hir::SemanticPrimitive::Float,
                )],
                variadic: None,
                result: Box::new(topaz_hir::SemanticType::Primitive(
                    topaz_hir::SemanticPrimitive::Float,
                )),
            },
            ambient: false,
        });
        typed.push_node(topaz_hir::TypedNode {
            module: "main".to_string(),
            kind: topaz_hir::TypedNodeKind::Binding,
            span: Span::new(FileId(0), 9, 13),
            ty: topaz_hir::SemanticType::Path,
            ambient: false,
        });
        let denied = collect_typed(CheckProfile::Bootstrap, Some(&typed));
        let rules = denied
            .iter()
            .filter_map(|finding| finding.rule)
            .collect::<Vec<_>>();
        assert!(rules.contains(&"bootstrap/no-float"), "{denied:#?}");
        assert!(rules.contains(&"bootstrap/no-resource"), "{denied:#?}");
    }

    #[test]
    fn bootstrap_inventory_carries_every_enforced_rule() {
        let inventory = include_str!("../../../contracts/compiler/v1/bootstrap-profile.json");
        for rule in [
            "bootstrap/no-ambient-type",
            "bootstrap/no-capability",
            "bootstrap/no-concurrency",
            "bootstrap/no-duration",
            "bootstrap/no-extern",
            "bootstrap/no-float",
            "bootstrap/no-host-leaf",
            "bootstrap/no-host-module",
            "bootstrap/no-resource",
            "bootstrap/no-test-framework",
            "bootstrap/requires-deterministic-build",
            "bootstrap/requires-locked-package",
        ] {
            assert!(inventory.contains(rule), "{rule}");
        }
    }
}
