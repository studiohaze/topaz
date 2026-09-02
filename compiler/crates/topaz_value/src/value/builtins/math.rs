use super::super::*;

// ----------------------------------------------------------------------------
// §8 (v5.4) the `Math` builtin namespace — the FIRST pure-compute stdlib slice.
//
// ★ FLOAT DETERMINISM. Every float op below routes through ONE shared leaf that
// the interpreter AND the emitted Rust BOTH call by name, so the result is
// byte-identical run≡build by construction (and, on a native decline → boxed,
// across all three columns). The ops use the Rust `std` `f64` primitives
// (`sqrt`/`abs`/`floor`/`ceil`/`round`/`is_nan`/`is_finite`) and string parse
// (`f64::from_str`), which are IEEE-754 correctly-rounded and platform-stable —
// the SAME std code runs in interp, boxed emit, and WASM, so libm is never
// called from two places. Decisions pinned here (and in the unit tests):
//   * `round` = round-half-AWAY-from-zero (Rust `f64::round`): `2.5→3.0`,
//     `-2.5→-3.0`, `0.5→1.0`. Total, deterministic.
//   * `sqrt` of a NEGATIVE (or NaN) input → value-level `Err` (NOT NaN, NOT a
//     fault) so a program can recover; `sqrt(-0.0) = -0.0` (IEEE, an `Ok`).
//   * `min`/`max` = the SHARED `<`/`>` ordering via `float_cmp` (NOT `f64::min`,
//     whose NaN-skipping is asymmetric): a NaN operand makes the comparison false,
//     so the result is the SECOND operand — `min(NaN, b) = b`, `min(a, NaN) = NaN`,
//     `max(NaN, b) = b`, `max(a, NaN) = NaN`. Asymmetric but fully deterministic.
//   * `parseFloat` = `f64::from_str` over the TRIMMED string → `Ok(f)` /
//     `Err(message)`; rejects the empty string and any non-numeric text. Rust std
//     accepts the spellings `inf`/`infinity`/`nan` (any case), but a Topaz text
//     parse must NOT mint a non-finite float, so a non-finite result is `Err` too —
//     only a FINITE literal yields `Ok`.

/// Pull the `f64` out of a `Value::Float`, else a GUARD_TYPE fault. The checker
/// already proves the argument is a `float` (the static-member scheme), so this
/// fault is reachable only on the `--unchecked` backstop — identical on both
/// engines because both call this one leaf.
pub(in crate::value) fn math_float_arg(
    arg: &Value,
    name: &str,
    span: Span,
) -> Result<f64, RtError> {
    match arg {
        Value::Float(x) => Ok(*x),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`Math.{name}` takes a `float`, found `{}`", other.kind()),
            span,
        )),
    }
}

/// §8 `Math.sqrt(x) -> Result<float, string>` — the principal square root. A
/// NEGATIVE or NaN input is a DOMAIN error returned as `Err` (never NaN, never a
/// fault), so the caller can handle it; `sqrt(-0.0) = -0.0` is a valid `Ok`.
pub fn builtin_math_sqrt(arg: Value, span: Span) -> Result<Value, RtError> {
    let x = math_float_arg(&arg, "sqrt", span)?;
    Ok(if x.is_nan() || x < 0.0 {
        Value::Err(Rc::new(Value::str(format!(
            "Math.sqrt: domain error (argument {x} is negative)"
        ))))
    } else {
        Value::Ok(Rc::new(Value::Float(x.sqrt())))
    })
}

/// §8 `Math.abs(x) -> float` — the absolute value. `abs(-0.0) = 0.0`,
/// `abs(NaN) = NaN` (IEEE).
pub fn builtin_math_abs(arg: Value, span: Span) -> Result<Value, RtError> {
    let x = math_float_arg(&arg, "abs", span)?;
    Ok(Value::Float(x.abs()))
}

/// §8 `Math.floor(x) -> float` — the largest integer ≤ x, as a float.
pub fn builtin_math_floor(arg: Value, span: Span) -> Result<Value, RtError> {
    let x = math_float_arg(&arg, "floor", span)?;
    Ok(Value::Float(x.floor()))
}

/// §8 `Math.ceil(x) -> float` — the smallest integer ≥ x, as a float.
pub fn builtin_math_ceil(arg: Value, span: Span) -> Result<Value, RtError> {
    let x = math_float_arg(&arg, "ceil", span)?;
    Ok(Value::Float(x.ceil()))
}

/// §8 `Math.round(x) -> float` — round half AWAY from zero (Rust `f64::round`):
/// `2.5→3.0`, `-2.5→-3.0`. Total + deterministic.
pub fn builtin_math_round(arg: Value, span: Span) -> Result<Value, RtError> {
    let x = math_float_arg(&arg, "round", span)?;
    Ok(Value::Float(x.round()))
}

/// §8 `Math.sin(x) -> float` — radians, routed through the single shared leaf.
pub fn builtin_math_sin(arg: Value, span: Span) -> Result<Value, RtError> {
    let x = math_float_arg(&arg, "sin", span)?;
    Ok(Value::Float(x.sin()))
}

/// §8 `Math.cos(x) -> float` — radians, routed through the single shared leaf.
pub fn builtin_math_cos(arg: Value, span: Span) -> Result<Value, RtError> {
    let x = math_float_arg(&arg, "cos", span)?;
    Ok(Value::Float(x.cos()))
}

/// §8 `Math.tan(x) -> float` — radians, routed through the single shared leaf.
pub fn builtin_math_tan(arg: Value, span: Span) -> Result<Value, RtError> {
    let x = math_float_arg(&arg, "tan", span)?;
    Ok(Value::Float(x.tan()))
}

/// §8 `Math.min(a, b) -> float` — the lesser by the SHARED `<` ordering
/// (`float_cmp`): if `a < b` return `a`, else `b`. A NaN operand makes `a < b`
/// false, so the result is the SECOND operand: `min(NaN, b) = b`, `min(a, NaN) =
/// NaN` — asymmetric but fully deterministic (the same on both engines).
pub fn builtin_math_min(a: Value, b: Value, span: Span) -> Result<Value, RtError> {
    let x = math_float_arg(&a, "min", span)?;
    let y = math_float_arg(&b, "min", span)?;
    Ok(Value::Float(if float_cmp(BinaryOp::Lt, x, y) {
        x
    } else {
        y
    }))
}

/// §8 `Math.max(a, b) -> float` — the greater by the SHARED `>` ordering
/// (`float_cmp`): if `a > b` return `a`, else `b`. A NaN operand makes `a > b`
/// false, so the result is the SECOND operand: `max(NaN, b) = b`, `max(a, NaN) =
/// NaN` — asymmetric but fully deterministic (the same on both engines).
pub fn builtin_math_max(a: Value, b: Value, span: Span) -> Result<Value, RtError> {
    let x = math_float_arg(&a, "max", span)?;
    let y = math_float_arg(&b, "max", span)?;
    Ok(Value::Float(if float_cmp(BinaryOp::Gt, x, y) {
        x
    } else {
        y
    }))
}

/// §8 `Math.isNaN(x) -> bool` — whether `x` is IEEE NaN.
pub fn builtin_math_is_nan(arg: Value, span: Span) -> Result<Value, RtError> {
    let x = math_float_arg(&arg, "isNaN", span)?;
    Ok(Value::Bool(x.is_nan()))
}

/// §8 `Math.isFinite(x) -> bool` — whether `x` is finite (not ±inf, not NaN).
pub fn builtin_math_is_finite(arg: Value, span: Span) -> Result<Value, RtError> {
    let x = math_float_arg(&arg, "isFinite", span)?;
    Ok(Value::Bool(x.is_finite()))
}

/// §8 `Math.parseFloat(s) -> Result<float, string>` — parse a FINITE float from
/// the TRIMMED string via `f64::from_str` (IEEE correctly-rounded, deterministic):
/// `Ok(f)` on success, `Err(message)` otherwise. The empty/whitespace string and
/// any non-numeric text are `Err`. ★ A text parse must NOT mint a NON-FINITE float:
/// Rust `f64::from_str` accepts the spellings `inf`/`infinity`/`nan` (any case), so
/// a successfully-parsed-but-non-finite result (`is_infinite() || is_nan()`) is
/// REJECTED as `Err` — only a finite float literal yields `Ok`. The argument must
/// be a `string` (the checker proves it; the non-string fault is the `--unchecked`
/// backstop).
pub fn builtin_math_parse_float(arg: Value, span: Span) -> Result<Value, RtError> {
    let s = match &arg {
        Value::Str(s) => s.clone(),
        other => {
            return Err(fault(
                codes::GUARD_TYPE,
                format!(
                    "`Math.parseFloat` takes a `string`, found `{}`",
                    other.kind()
                ),
                span,
            ));
        }
    };
    let parse_err = || {
        Value::Err(Rc::new(Value::str(format!(
            "Math.parseFloat: could not parse `{s}` as a float"
        ))))
    };
    Ok(match s.trim().parse::<f64>() {
        // Reject `inf`/`Infinity`/`nan` (any case) and any other non-finite parse —
        // a Topaz text parse never produces ±inf or NaN.
        Ok(f) if f.is_finite() => Value::Ok(Rc::new(Value::Float(f))),
        _ => parse_err(),
    })
}
