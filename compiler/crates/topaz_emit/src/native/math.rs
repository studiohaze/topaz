use super::*;

pub(super) fn bind_math_args(
    args: &[CallArg],
    params: &[&str],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    span: Span,
) -> Result<(String, Vec<String>), EmitError> {
    if args.len() != params.len() {
        return Err(decline("a Math call with a mismatched argument count").at(span));
    }
    let mut slots: Vec<Option<String>> = vec![None; params.len()];
    let mut next_positional = 0usize;
    let mut saw_named = false;
    let mut bindings = String::new();

    for (idx, arg) in args.iter().enumerate() {
        let (param_idx, expr) = match arg {
            CallArg::Positional(expr) => {
                if saw_named || next_positional >= params.len() {
                    return Err(
                        decline("a positional Math argument after a named argument").at(expr.span)
                    );
                }
                let param_idx = next_positional;
                next_positional += 1;
                (param_idx, expr)
            }
            CallArg::Named { name, value } => {
                saw_named = true;
                let n = text(ctx.src, name.span);
                let Some(param_idx) = params.iter().position(|param| *param == n) else {
                    return Err(decline("a named Math argument with no parameter").at(name.span));
                };
                if slots[param_idx].is_some() {
                    return Err(decline("a Math argument supplied twice").at(name.span));
                }
                (param_idx, value)
            }
            CallArg::Spread(expr) => return Err(decline("a spread Math argument").at(expr.span)),
        };

        let low = emit_expr(expr, ctx, scope)?;
        if low.ty != NativeTy::F64 {
            return Err(decline("a non-float Math argument").at(expr.span));
        }
        let temp = format!("__math_arg_{idx}");
        bindings.push_str(&format!("let {temp} = {}; ", low.rs));
        slots[param_idx] = Some(temp);
    }

    let ordered = slots
        .into_iter()
        .map(|slot| slot.ok_or_else(|| decline("a Math call with a mismatched argument count")))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((bindings, ordered))
}

pub(super) fn lower_math_block(bindings: String, body: String, ty: NativeTy) -> Lowered {
    if bindings.is_empty() {
        Lowered { rs: body, ty }
    } else {
        Lowered {
            rs: format!("{{ {bindings}{body} }}"),
            ty,
        }
    }
}

/// Lower total scalar `Math.*` calls (or a `std.math` namespace alias) into the
/// same Rust f64 operations the shared value leaves use. Result-returning helpers
/// (`sqrt`, `parseFloat`) remain boxed-only and therefore decline here.
pub(super) fn emit_math_call(
    callee: &Expr,
    args: &[topaz_hir::emission::CallArg],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    span: Span,
) -> Result<Option<Lowered>, EmitError> {
    let ExprKind::Member { object, field } = &callee.kind else {
        return Ok(None);
    };
    let ExprKind::Ident = &object.kind else {
        return Ok(None);
    };
    let namespace = text(ctx.src, object.span);
    if scope.iter().any(|local| local.name == namespace) || !ctx.is_math_namespace(namespace) {
        return Ok(None);
    }

    let member = text(ctx.src, field.span);
    let one_float = || -> Result<(String, String), EmitError> {
        let (bindings, ordered) = bind_math_args(args, &["x"], ctx, scope, span)?;
        let mut ordered = ordered.into_iter();
        let x = ordered
            .next()
            .ok_or_else(|| decline("a Math call with a mismatched argument count").at(span))?;
        Ok((bindings, x))
    };
    let two_float = || -> Result<(String, String, String), EmitError> {
        let (bindings, ordered) = bind_math_args(args, &["a", "b"], ctx, scope, span)?;
        let mut ordered = ordered.into_iter();
        let a = ordered
            .next()
            .ok_or_else(|| decline("a Math call with a mismatched argument count").at(span))?;
        let b = ordered
            .next()
            .ok_or_else(|| decline("a Math call with a mismatched argument count").at(span))?;
        Ok((bindings, a, b))
    };

    let lowered = match member {
        "abs" => {
            let (bindings, x) = one_float()?;
            lower_math_block(bindings, format!("({x}).abs()"), NativeTy::F64)
        }
        "floor" => {
            let (bindings, x) = one_float()?;
            lower_math_block(bindings, format!("({x}).floor()"), NativeTy::F64)
        }
        "ceil" => {
            let (bindings, x) = one_float()?;
            lower_math_block(bindings, format!("({x}).ceil()"), NativeTy::F64)
        }
        "round" => {
            let (bindings, x) = one_float()?;
            lower_math_block(bindings, format!("({x}).round()"), NativeTy::F64)
        }
        "sin" => {
            let (bindings, x) = one_float()?;
            lower_math_block(bindings, format!("({x}).sin()"), NativeTy::F64)
        }
        "cos" => {
            let (bindings, x) = one_float()?;
            lower_math_block(bindings, format!("({x}).cos()"), NativeTy::F64)
        }
        "tan" => {
            let (bindings, x) = one_float()?;
            lower_math_block(bindings, format!("({x}).tan()"), NativeTy::F64)
        }
        "isNaN" => {
            let (bindings, x) = one_float()?;
            lower_math_block(bindings, format!("({x}).is_nan()"), NativeTy::Bool)
        }
        "isFinite" => {
            let (bindings, x) = one_float()?;
            lower_math_block(bindings, format!("({x}).is_finite()"), NativeTy::Bool)
        }
        "min" => {
            let (bindings, a, b) = two_float()?;
            lower_math_block(
                bindings,
                format!(
                    "{{ let __a = {a}; let __b = {b}; if float_cmp(BinaryOp::Lt, __a, __b) {{ __a }} else {{ __b }} }}"
                ),
                NativeTy::F64,
            )
        }
        "max" => {
            let (bindings, a, b) = two_float()?;
            lower_math_block(
                bindings,
                format!(
                    "{{ let __a = {a}; let __b = {b}; if float_cmp(BinaryOp::Gt, __a, __b) {{ __a }} else {{ __b }} }}"
                ),
                NativeTy::F64,
            )
        }
        "sqrt" | "parseFloat" => {
            return Err(decline("a Result-returning Math call").at(callee.span));
        }
        _ => return Ok(None),
    };
    Ok(Some(lowered))
}

// ----------------------------------------------------------------------------
// Array<scalar> READ boundary — a boxed `Value::Array` of scalar elements whose
// element reads + `.length` lower native. The array stays boxed; only the reads
// cross into native scalars, through the shared `index_value`/`member_value`
// leaves (so the OOB fault + length value are byte-identical to interp+boxed).
// ----------------------------------------------------------------------------

/// If `pattern` is a typed binding `name: Array<E>` with `E` a CONCRETE scalar,
/// the element [`NativeTy`]; otherwise `None`. This is the syntactic signal that
/// a `let` introduces a native scalar-array boundary local — and a clean check
/// has verified the elements really are `E`, so reading them as `E` is sound.
pub(super) fn typed_scalar_array(
    pattern: &Pattern,
    stmt_ty: Option<&topaz_hir::emission::Type>,
    src: &LoweredText,
) -> Option<NativeTy> {
    if let PatternKind::Typed { ty, .. } = &pattern.kind {
        return scalar_array_type(ty, src);
    }
    stmt_ty.and_then(|ty| scalar_array_type(ty, src))
}

/// If `ty` is `Array<E>` with `E` a concrete native scalar, return `E`.
pub(super) fn scalar_array_type(
    ty: &topaz_hir::emission::Type,
    src: &LoweredText,
) -> Option<NativeTy> {
    // `Array<E>` — a `Named` type "Array" with exactly one concrete-scalar arg.
    let TypeKind::Named { name, args } = &ty.kind else {
        return None;
    };
    if text(src, name.span) != "Array" || args.len() != 1 {
        return None;
    }
    match scalar_of_type(&args[0], src)? {
        elem @ (NativeTy::I64 | NativeTy::F64 | NativeTy::Bool | NativeTy::Str) => Some(elem),
        NativeTy::Unit => None,
    }
}

pub(super) fn std_math_namespace_alias(imp: &ast::ImportItem, src: &LoweredText) -> Option<String> {
    let identity = imp
        .path
        .segments
        .iter()
        .map(|segment| text(src, segment.span))
        .collect::<Vec<_>>()
        .join(".");
    if identity != "std.math" {
        return None;
    }
    let ast::ImportKind::Namespace { alias } = &imp.kind else {
        return None;
    };
    let name = alias
        .as_ref()
        .map(|id| text(src, id.span))
        .unwrap_or_else(|| text(src, imp.path.segments.last().expect("non-empty path").span));
    Some(name.to_string())
}

pub(super) fn native_unbox_helper(ty: NativeTy) -> Option<&'static str> {
    match ty {
        NativeTy::I64 => Some("native_unbox_int"),
        NativeTy::F64 => Some("native_unbox_float"),
        NativeTy::Bool => Some("native_unbox_bool"),
        NativeTy::Str => Some("native_unbox_string"),
        NativeTy::Unit => None,
    }
}

// ----------------------------------------------------------------------------
// Operator lowering — through the SHARED checked-arith leaf for byte identity.
// ----------------------------------------------------------------------------

/// Lower a unary operator over a native scalar. `-x` / `+x` on int route
/// through the shared `int_neg` leaf (so `-i64::MIN` faults TPZ4004 at `span`);
/// float negation is bare IEEE; `!b` is bare bool. Everything else refuses.
pub(super) fn lower_unary(op: UnaryOp, v: &Lowered, span: Span) -> Result<Lowered, EmitError> {
    match (op, v.ty) {
        (UnaryOp::Plus, NativeTy::I64) => Ok(Lowered {
            rs: v.rs.clone(),
            ty: NativeTy::I64,
        }),
        (UnaryOp::Plus, NativeTy::F64) => Ok(Lowered {
            rs: v.rs.clone(),
            ty: NativeTy::F64,
        }),
        (UnaryOp::Minus, NativeTy::I64) => Ok(Lowered {
            rs: format!("int_neg({}, {})?", v.rs, emit_span(span)),
            ty: NativeTy::I64,
        }),
        (UnaryOp::Minus, NativeTy::F64) => Ok(Lowered {
            rs: format!("(-({}))", v.rs),
            ty: NativeTy::F64,
        }),
        (UnaryOp::Not, NativeTy::Bool) => Ok(Lowered {
            rs: format!("(!({}))", v.rs),
            ty: NativeTy::Bool,
        }),
        _ => Err(decline("an unsupported unary operation")),
    }
}

/// Lower a binary operator over two native scalars THROUGH the shared
/// `topaz_value` leaf, so arithmetic + comparison + their faults are
/// byte-identical to the interpreter (same helper, same span). The operand types
/// must MATCH (int+int, float+float, bool==bool) exactly as `binary_value`'s arms
/// require — a mixed/unsupported pair refuses.
pub(super) fn lower_binary(
    op: BinaryOp,
    l: &Lowered,
    r: &Lowered,
    span: Span,
) -> Result<Lowered, EmitError> {
    use BinaryOp::*;
    let span_rs = emit_span(span);
    // Integer arithmetic → the shared checked leaf (returns `Result<i64, _>`).
    if l.ty == NativeTy::I64 && r.ty == NativeTy::I64 {
        let helper = match op {
            Add => Some("int_add"),
            Sub => Some("int_sub"),
            Mul => Some("int_mul"),
            Div => Some("int_div"),
            Rem => Some("int_rem"),
            Pow => Some("int_pow"),
            _ => None,
        };
        if let Some(h) = helper {
            return Ok(Lowered {
                rs: format!("{h}({}, {}, {span_rs})?", l.rs, r.rs),
                ty: NativeTy::I64,
            });
        }
        if matches!(op, Lt | Le | Gt | Ge) {
            return Ok(Lowered {
                rs: format!("int_cmp(BinaryOp::{op:?}, {}, {})", l.rs, r.rs),
                ty: NativeTy::Bool,
            });
        }
        if matches!(op, Eq | Ne) {
            // `==`/`!=` over int: bare Rust, identical to `values_equal`'s Int arm.
            let rust = if matches!(op, Eq) { "==" } else { "!=" };
            return Ok(Lowered {
                rs: format!("(({}) {rust} ({}))", l.rs, r.rs),
                ty: NativeTy::Bool,
            });
        }
    }
    // Float arithmetic → the shared IEEE leaf (`%` is int-only, so it is NOT
    // routed here — a `Rem` on floats refuses, exactly as `binary_value` faults).
    if l.ty == NativeTy::F64 && r.ty == NativeTy::F64 {
        if matches!(op, Add | Sub | Mul | Div | Pow) {
            return Ok(Lowered {
                rs: format!("float_arith(BinaryOp::{op:?}, {}, {})", l.rs, r.rs),
                ty: NativeTy::F64,
            });
        }
        if matches!(op, Lt | Le | Gt | Ge) {
            return Ok(Lowered {
                rs: format!("float_cmp(BinaryOp::{op:?}, {}, {})", l.rs, r.rs),
                ty: NativeTy::Bool,
            });
        }
        if matches!(op, Eq | Ne) {
            // IEEE equality (`NaN != NaN`): bare Rust `==`/`!=` on f64 matches
            // `values_equal`'s Float arm exactly.
            let rust = if matches!(op, Eq) { "==" } else { "!=" };
            return Ok(Lowered {
                rs: format!("(({}) {rust} ({}))", l.rs, r.rs),
                ty: NativeTy::Bool,
            });
        }
    }
    // Bool equality (`==`/`!=`): bare Rust, identical to `values_equal`'s Bool arm.
    if l.ty == NativeTy::Bool && r.ty == NativeTy::Bool && matches!(op, Eq | Ne) {
        let rust = if matches!(op, Eq) { "==" } else { "!=" };
        return Ok(Lowered {
            rs: format!("(({}) {rust} ({}))", l.rs, r.rs),
            ty: NativeTy::Bool,
        });
    }
    // String concatenation and lexicographic comparison. `String` is non-Copy, so
    // concatenation binds both sides once before building the result.
    if l.ty == NativeTy::Str && r.ty == NativeTy::Str {
        if matches!(op, Add) {
            return Ok(Lowered {
                rs: format!(
                    "{{ let __a = {}; let __b = {}; let mut __s = String::with_capacity(__a.len() + __b.len()); __s.push_str(&__a); __s.push_str(&__b); __s }}",
                    l.rs, r.rs
                ),
                ty: NativeTy::Str,
            });
        }
        if matches!(op, Eq | Ne | Lt | Le | Gt | Ge) {
            let rust = match op {
                Eq => "==",
                Ne => "!=",
                Lt => "<",
                Le => "<=",
                Gt => ">",
                Ge => ">=",
                _ => unreachable!(),
            };
            return Ok(Lowered {
                rs: format!("(({}) {rust} ({}))", l.rs, r.rs),
                ty: NativeTy::Bool,
            });
        }
    }
    Err(decline("an unsupported binary operation"))
}

// ----------------------------------------------------------------------------
// Small AST helpers.
// ----------------------------------------------------------------------------

/// The bound name (and its identifier span) of a SIMPLE identifier `let` pattern
/// (`x` or `x: T`), or `None` for a wildcard/destructuring/other pattern the
/// native island does not model. The span is the BINDING NAME's span — exactly
/// where the checker records the typed local — so the typed-HIR cross-check keys
/// match (a `Typed` pattern spans `x: T`, but the local is recorded at `x`).
pub(super) fn simple_binding<'a>(
    pattern: &'a Pattern,
    src: &'a LoweredText,
) -> Option<(&'a str, Span)> {
    match &pattern.kind {
        PatternKind::Binding(name) => Some((text(src, name.span), name.span)),
        PatternKind::Typed { name, .. } => Some((text(src, name.span), name.span)),
        _ => None,
    }
}

/// A concrete scalar type annotation → its [`NativeTy`], or `None` for anything
/// non-scalar (a constructor, alias, generic, qualified, function, union, …).
/// Only the bare keyword names `int`/`float`/`bool` and the unit `()` map.
pub(super) fn scalar_of_type(
    ty: &topaz_hir::emission::Type,
    src: &LoweredText,
) -> Option<NativeTy> {
    match &ty.kind {
        TypeKind::Named { name, args } if args.is_empty() => match text(src, name.span) {
            "int" => Some(NativeTy::I64),
            "float" => Some(NativeTy::F64),
            "bool" => Some(NativeTy::Bool),
            "string" => Some(NativeTy::Str),
            _ => None,
        },
        // The unit type is spelled `()` — its `TypeKind` is rendered specially;
        // probe the source text for the exact unit spelling.
        _ if text(src, ty.span) == "()" => Some(NativeTy::Unit),
        _ => None,
    }
}
