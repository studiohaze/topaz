use super::*;

/// The LAST index segment of an assignment-target chain (the
/// member suffix past it is purely record fields), or None for an
/// index-free chain.
pub(super) fn last_index_segment(target: &ast::Expr) -> Option<&ast::Expr> {
    match &target.kind {
        ast::ExprKind::Member { object, .. } => last_index_segment(object),
        ast::ExprKind::Index { .. } => Some(target),
        _ => None,
    }
}

/// The statement an `export` wraps (a zero-runtime wrapper); any other
/// statement is itself.
pub(super) fn top_inner(stmt: &ast::Stmt) -> &ast::StmtKind {
    match &stmt.kind {
        ast::StmtKind::Export(inner) => &inner.kind,
        _ => &stmt.kind,
    }
}

/// The Ident root of an assignment/mutation chain, walking through
/// record members AND collection indices (§4/§9: the in-place write
/// requires this binding to be mutable). None when the chain does
/// not bottom out at an identifier.
/// Whether an assignment target routes through optional access
/// (`?.`), which is conditional and not assignable (§4).
pub(super) fn target_has_optional(target: &ast::Expr) -> bool {
    match &target.kind {
        ast::ExprKind::OptionalAccess { .. } => true,
        ast::ExprKind::Member { object, .. }
        | ast::ExprKind::Index { object, .. }
        | ast::ExprKind::Paren(object) => target_has_optional(object),
        _ => false,
    }
}

pub(super) fn assignment_root(target: &ast::Expr) -> Option<&ast::Expr> {
    match &target.kind {
        ast::ExprKind::Ident => Some(target),
        ast::ExprKind::Member { object, .. }
        | ast::ExprKind::Index { object, .. }
        | ast::ExprKind::Paren(object) => assignment_root(object),
        _ => None,
    }
}

/// Collects the distinct inference var indices of a type, in order.
pub(super) fn collect_vars_into(t: &Type, index: &mut Vec<u32>) {
    t.for_each_component(|component| {
        if let Type::Var(i) = component
            && !index.contains(i)
        {
            index.push(*i);
        }
    });
}

/// Rewrites each var to its position in `index` (dense join space).
pub(super) fn remap_vars(t: &Type, index: &[u32]) -> Type {
    t.transform_components(&mut |component| match component {
        Type::Var(variable) => {
            let position = index
                .iter()
                .position(|candidate| candidate == variable)
                .expect("collected var");
            Some(Type::Var(position as u32))
        }
        _ => None,
    })
}

/// Whether the type contains the given inference var (the occurs
/// check for partial-type bindings).
pub(super) fn contains_var_index(t: &Type, i: u32) -> bool {
    t.any_component(&mut |component| matches!(component, Type::Var(j) if *j == i))
}

/// Contains a synthetic projection skolem (`FieldOf<T, x>` &c, whose
/// id self-registered in `projection_ids`): an unnameable rigid type
/// that must not escape into an inferred signature.
pub(super) fn contains_projection(t: &Type, ids: &[u32]) -> bool {
    t.any_component(
        &mut |component| matches!(component, Type::Skolem { id, .. } if ids.contains(id)),
    )
}

/// Contains a rigid generic anywhere: a `Skolem` (a real type parameter `T` OR a
/// synthetic projection like `ElemOf<T>`) or a `Foreign` type. Used to decide
/// when an iterable element is GROUND-rigid and so must win over a `ctx`
/// pre-binding of the element var (`filter`'s shared input/output var).
pub(super) fn contains_rigid(t: &Type) -> bool {
    t.any_component(&mut |component| {
        matches!(component, Type::Skolem { .. } | Type::Foreign { .. })
    })
}

/// Replaces every synthetic projection skolem (id in `ids`) with
/// `Unknown`, preserving the rest of the type's structure (concrete
/// members, parameters, the base `T`). Gradualizes a leaked
/// projection out of a published signature without discarding the
/// nameable parts around it.
pub(super) fn strip_projections(t: &Type, ids: &[u32]) -> Type {
    t.transform_components(&mut |component| match component {
        Type::Skolem { id, .. } if ids.contains(id) => Some(Type::Unknown),
        _ => None,
    })
}

/// Contains a literal `Unknown` (not an inference var): the ambient
/// silence test for branch joins.
pub(super) fn contains_true_unknown(t: &Type) -> bool {
    t.any_component(&mut |component| matches!(component, Type::Unknown))
}

/// The identifier a value expression aliases, through parens and
/// single-expression blocks.
pub(super) fn alias_source(e: &ast::Expr) -> Option<Span> {
    match &e.kind {
        ast::ExprKind::Ident => Some(e.span),
        ast::ExprKind::Paren(inner) => alias_source(inner),
        ast::ExprKind::Block(b) if b.stmts.is_empty() => b.tail.as_deref().and_then(alias_source),
        _ => None,
    }
}

/// Rewrites body skolems back to their declaration's scheme vars
/// (omitted-return inference: the signature speaks in `Var(i)`).
pub(super) fn skolems_to_vars(t: &Type, map: &[(u32, u32)]) -> Type {
    t.transform_components(&mut |component| match component {
        Type::Skolem { id, .. } => map
            .iter()
            .find(|(skolem, _)| skolem == id)
            .map(|(_, variable)| Type::Var(*variable)),
        _ => None,
    })
}

/// A block diverges when its final statement is a return (no tail)
/// or its tail expression diverges.
pub(super) fn block_diverges(b: &ast::Block) -> bool {
    match &b.tail {
        Some(tail) => arm_diverges(tail),
        None => matches!(
            b.stmts.last().map(|s| &s.kind),
            Some(ast::StmtKind::Return(_))
        ),
    }
}

/// A branch/arm body that syntactically always returns diverges:
/// a return-terminal block, an if whose branches all diverge, or a
/// match whose arms all diverge.
pub(super) fn arm_diverges(e: &ast::Expr) -> bool {
    match &e.kind {
        ast::ExprKind::Block(b) => block_diverges(b),
        ast::ExprKind::Paren(inner) => arm_diverges(inner),
        ast::ExprKind::If {
            then_block,
            else_branch: Some(else_branch),
            ..
        } => block_diverges(then_block) && arm_diverges(else_branch),
        ast::ExprKind::Match { cases, .. } => {
            !cases.is_empty()
                && cases.iter().all(|case| match &case.body {
                    ast::CaseArmBody::Return { .. } => true,
                    ast::CaseArmBody::Expr(e) => arm_diverges(e),
                })
        }
        _ => false,
    }
}

/// The single source of truth for "does this receiver decidably HAVE `member`?"
/// shared by single-receiver typing and union-arm probing. It NEVER emits.
/// `Some(true)` = the member resolves; `Some(false)` = the receiver is
/// member-closed and the member is decidably ABSENT (the caller emits TPZ5006);
/// `None` = staged — an opaque receiver (`Unknown`/`Var`/…) whose membership the
/// checker cannot decide. A `Type::Union` ignores its `null` arm (nullability is
/// a `?.`/runtime concern) and decidably LACKS a member iff any member-closed
/// non-null arm lacks it — that arm would runtime-fault.
/// A receiver that is opaque-but-RIGID: a bare generic (`Skolem`/`Foreign`) or a
/// union with any rigid arm (recursively). The projection rule maps a rigid member/call
/// result for these instead of tainting to `Unknown`, so a member access or call
/// on a generic cannot silently discharge a concrete boundary. Truly gradual
/// receivers (`Unknown`/`Var`) are excluded — they stay gradual.
pub(super) fn is_rigid_or_union_rigid(ty: &Type) -> bool {
    match ty {
        Type::Skolem { .. } | Type::Foreign { .. } => true,
        Type::Union(arms) => arms.iter().any(is_rigid_or_union_rigid),
        _ => false,
    }
}

/// A union whose receiver MIGHT be a concrete arm on which `member` is a §9
/// mutator (e.g. `Array<int> | T` calling `push`). The whole-union
/// `builtins::is_mutator` misses this, so the mutation-root check would be
/// skipped on the projected call path — this restores it.
pub(super) fn union_arm_is_mutator(ty: &Type, member: &str) -> bool {
    matches!(ty, Type::Union(arms) if arms.iter().any(|a| {
        !matches!(a, Type::Literal(Lit::Null))
            && !is_rigid_or_union_rigid(a)
            && builtins::is_mutator(a, member)
    }))
}

/// §4 (v5.4) the NOMINAL id of a receiver type — the declaring type's name for a
/// record/enum/newtype, else `None`. Used to pick a user receiver method at a call
/// site (the checker's twin of the runtime nominal-id dispatch).
pub(super) fn nominal_type_id(ty: &Type) -> Option<String> {
    match ty {
        Type::NominalRecord { base, args }
        | Type::Enum { base, args }
        | Type::Newtype { base, args } => Some(nominal_instance_id(base, args)),
        _ => None,
    }
}

pub(super) fn nominal_ctx_matches(base: &str, ctx_base: &str) -> bool {
    ctx_base == base
}

pub(super) fn receiver_has_member(receiver: &Type, member: &str) -> Option<bool> {
    if builtins::receiver_member(receiver, member).is_some() {
        return Some(true);
    }
    match receiver {
        Type::Record(fields) => Some(fields.iter().any(|(n, _)| n == member)),
        // §3 (v5.4) a newtype is member-closed: its ONLY member is `.value()`, so an
        // unknown member is a decidable absence (matching `member_type`).
        Type::Newtype { .. } => Some(member == "value"),
        // Member-closed builtin/scalar receivers: a miss is a decidable absence,
        // in both the widened (`Prim`) and the literal forms. Every scalar
        // exposes a FIXED member set, so an unknown member is a static error
        // matching the interpreter's runtime fault: `int`/`float`/`bool` expose
        // NOTHING (C3); `string`'s SOLE member is the `scalars()` method (SPEC
        // §22.2 — frozen), so any other `string` member/call like `s.contains(..)`
        // does not exist in v5.2 and faults at runtime, hence a decidable absence.
        // Functions, `unit`, and `null` are likewise member-closed (they expose
        // NOTHING), so `f.x` / `().x` / `null.x` is a decidable absence. `null`
        // is member-closed on the PLAIN access path: a bare `v.x` on a value that
        // may be `null` faults at runtime, so strict typing rejects it and
        // demands `?.`, `??`, or a type
        // pattern — exactly as a plain member on `Option<X>` already does. The
        // `?.` path never reaches here: `optional_member` strips the optional via
        // `unwrap_optional` first.
        Type::Ctor(_, _)
        | Type::File
        | Type::Template
        | Type::Func { .. }
        | Type::Prim(Prim::String | Prim::Int | Prim::Float | Prim::Bool | Prim::Unit)
        | Type::Literal(Lit::Str(_) | Lit::Int(_) | Lit::Float(_) | Lit::Bool(_) | Lit::Null) => {
            Some(false)
        }
        // A union decidably LACKS the member iff any arm lacks it (including a
        // `null` arm — see above); HAS it iff every arm has it; else undecidable.
        Type::Union(arms) => {
            let mut undecidable = false;
            for arm in arms {
                match receiver_has_member(arm, member) {
                    Some(false) => return Some(false),
                    None => undecidable = true,
                    Some(true) => {}
                }
            }
            if undecidable { None } else { Some(true) }
        }
        // Opaque receivers stay staged: `Unknown`, `Var`, `null`, and
        // foreign/skolem types.
        _ => None,
    }
}

/// §3 (v5.3): whether a single-payload type is a UNION that DIRECTLY contains a
/// user enum (`Color | int`). Such a payload is rejected because a
/// bare subpattern there (`case Wrap(Red)`) is ambiguous between an enum-variant
/// match and a plain binding, which the checker (nominal, type-based) and the
/// runtime/emit (value-based) resolve differently, so it would diverge run≢build.
/// A union with NO enum member (`int | string`) is fine: a bare subpattern is an
/// unambiguous binding.
pub(super) fn union_payload_contains_enum(ty: &Type) -> bool {
    matches!(ty, Type::Union(members) if members.iter().any(|m| matches!(m, Type::Enum { .. })))
}

/// Types where pattern impossibility and overlap are decidable:
/// fully concrete structure, with no Foreign, Skolem, Unknown,
/// function, or opaque component inside (CDR-004 §3).
pub(super) fn decidable_type(t: &Type) -> bool {
    match t {
        Type::Prim(_) | Type::Literal(_) => true,
        Type::Union(members) => members.iter().all(decidable_type),
        Type::Ctor(_, args) => args.iter().all(decidable_type),
        Type::Record(fields) => fields.iter().all(|(_, t)| decidable_type(t)),
        _ => false,
    }
}

/// Whether two decidable types share any inhabitant. Same-kind
/// containers conservatively overlap — `Option<int>` and
/// `Option<string>` share `None`, arrays share `[]` — and records
/// overlap on identical field sets with overlapping field types.
/// A literal-valued expression with no interpolation — usable as a record
/// discriminant whose type is known without side effects.
pub(super) fn is_plain_literal(expr: &ast::Expr) -> bool {
    match &expr.kind {
        ast::ExprKind::Int
        | ast::ExprKind::Float
        | ast::ExprKind::Null
        | ast::ExprKind::Unit
        | ast::ExprKind::Bool(_)
        | ast::ExprKind::Duration(_) => true,
        ast::ExprKind::String(s) => !s
            .parts
            .iter()
            .any(|p| matches!(p, ast::StringPart::Interpolation(_))),
        _ => false,
    }
}

pub(super) fn type_overlap(a: &Type, b: &Type) -> bool {
    if is_subtype(a, b) || is_subtype(b, a) {
        return true;
    }
    match (a, b) {
        (Type::Union(members), _) => members.iter().any(|m| type_overlap(m, b)),
        (_, Type::Union(members)) => members.iter().any(|m| type_overlap(a, m)),
        (Type::Ctor(ka, _), Type::Ctor(kb, _)) => ka == kb,
        (Type::Record(fa), Type::Record(fb)) => {
            fa.len() == fb.len()
                && fa
                    .iter()
                    .zip(fb.iter())
                    .all(|((na, ta), (nb, tb))| na == nb && type_overlap(ta, tb))
        }
        _ => false,
    }
}

/// Returns a representative pair from two disconnected components of the
/// static-overlap graph. `Unknown`/`Var` is a gradual connector, matching the
/// or-pattern agreement rule that it must not manufacture an order-dependent
/// TPZ5711. `None` means every binding type belongs to one connected component.
pub(super) fn disconnected_overlap_pair(types: &[Type]) -> Option<(Type, Type)> {
    if types.len() < 2 {
        return None;
    }
    let mut reached = vec![false; types.len()];
    let mut stack = vec![0usize];
    reached[0] = true;
    while let Some(index) = stack.pop() {
        for candidate in 0..types.len() {
            if reached[candidate] {
                continue;
            }
            if types[index].has_unknown()
                || types[candidate].has_unknown()
                || type_overlap(&types[index], &types[candidate])
            {
                reached[candidate] = true;
                stack.push(candidate);
            }
        }
    }
    reached
        .iter()
        .position(|seen| !seen)
        .map(|index| (types[0].clone(), types[index].clone()))
}

/// Lowers every remaining inference `Var(_)` to `Unknown`, preserving
/// the surrounding type shape. Used to keep a polymorphic constructor
/// value's Ctor (e.g. `None`'s `Option<_>`) visible when its element
/// type stays unsolved, instead of collapsing the whole value to a
/// bare `Unknown` and losing the receiver shape for member dispatch.
pub(super) fn unknown_for_vars(ty: &Type) -> Type {
    ty.transform_components(&mut |component| {
        matches!(component, Type::Var(_)).then_some(Type::Unknown)
    })
}

/// Replaces solved `Var(i)` with their bindings.
pub(super) fn substitute(ty: &Type, subst: &[Option<Type>]) -> Type {
    ty.transform_components(&mut |component| match component {
        Type::Var(index) => Some(
            subst
                .get(*index as usize)
                .and_then(Clone::clone)
                .unwrap_or(Type::Var(*index)),
        ),
        _ => None,
    })
}

/// Structural one-way unification: variables in `pattern` bind to the
/// corresponding parts of `actual`. Argument-driven bindings widen
/// literals (`widen: true`); contextual bindings keep them — an
/// expected `Option<"open">` must bind T to the literal. Unknown
/// never binds.
pub(super) fn unify_with(pattern: &Type, actual: &Type, subst: &mut [Option<Type>], widen: bool) {
    match (pattern, actual) {
        (Type::Var(i), t) => {
            // True Unknown never binds; a PARTIAL (var-carrying)
            // type may, occurs-checked, so `Some([])` keeps its
            // array shape through the scheme variable.
            if !contains_true_unknown(t)
                && !contains_var_index(t, *i)
                && let Some(slot) = subst.get_mut(*i as usize)
                && slot.is_none()
            {
                *slot = Some(if widen { t.clone().widen() } else { t.clone() });
            }
        }
        (Type::Union(ps), t) => {
            for p in ps {
                unify_with(p, t, subst, widen);
            }
        }
        (Type::Ctor(ca, pa), Type::Ctor(cb, ab)) if ca == cb && pa.len() == ab.len() => {
            for (p, a) in pa.iter().zip(ab.iter()) {
                unify_with(p, a, subst, widen);
            }
        }
        (Type::Enum { base: pb, args: pa }, Type::Enum { base: ab, args: aa })
            if pb == ab && pa.len() == aa.len() =>
        {
            for (p, a) in pa.iter().zip(aa.iter()) {
                unify_with(p, a, subst, widen);
            }
        }
        (
            Type::NominalRecord { base: pb, args: pa },
            Type::NominalRecord { base: ab, args: aa },
        ) if pb == ab && pa.len() == aa.len() => {
            for (p, a) in pa.iter().zip(aa.iter()) {
                unify_with(p, a, subst, widen);
            }
        }
        (Type::Newtype { base: pb, args: pa }, Type::Newtype { base: ab, args: aa })
            if pb == ab && pa.len() == aa.len() =>
        {
            for (p, a) in pa.iter().zip(aa.iter()) {
                unify_with(p, a, subst, widen);
            }
        }
        (Type::Record(pf), Type::Record(af)) if pf.len() == af.len() => {
            for ((pn, pt), (an, at)) in pf.iter().zip(af.iter()) {
                if pn == an {
                    unify_with(pt, at, subst, widen);
                }
            }
        }
        (
            Type::Func {
                params: pp,
                variadic: pv,
                ret: pr,
            },
            Type::Func {
                params: ap,
                variadic: av,
                ret: ar,
            },
        ) => {
            for (p, a) in pp.iter().zip(ap.iter()) {
                unify_with(p, a, subst, widen);
            }
            if let (Some(p), Some(a)) = (pv, av) {
                unify_with(p, a, subst, widen);
            }
            unify_with(pr, ar, subst, widen);
        }
        // A union context cannot pick a member without backtracking;
        // leaving the variables unsolved routes to the §22.1 error at
        // the context site instead of guessing a member.
        _ => {}
    }
}

pub(super) fn unify(pattern: &Type, actual: &Type, subst: &mut [Option<Type>]) {
    unify_with(pattern, actual, subst, true);
}
