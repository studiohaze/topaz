use super::*;

// §2 operator semantics — the SINGLE shared implementation both
// the interpreter and emitted code call, so arithmetic, comparison,
// and their §13a faults cannot drift between engines (CDR-006 §2).

/// §5 condition guard — an `if`/`while` condition must be `bool`. The
/// SINGLE shared check both engines call, so a non-`bool` condition
/// faults identically (`GUARD_TYPE`, same message, same span) whether
/// the program runs on the interpreter or in emitted code (CDR-006 §2).
/// `keyword` is the construct the message names (`"if"` / `"while"`).
pub fn condition_bool(value: &Value, keyword: &str, span: Span) -> Result<bool, RtError> {
    match value {
        Value::Bool(b) => Ok(*b),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`{keyword}` condition must be `bool`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

/// §5 a `case` guard's value must be `bool` (the case matches only on `true`;
/// a non-`bool` faults). Its own leaf — distinct from [`condition_bool`] — so
/// the guard's GUARD_TYPE message matches the interpreter's `KMatchGuard`
/// EXACTLY; both engines call this so the message and span cannot drift.
pub fn case_guard_bool(value: &Value, span: Span) -> Result<bool, RtError> {
    match value {
        Value::Bool(b) => Ok(*b),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`case` guard must be `bool`, found `{}`", other.kind()),
            span,
        )),
    }
}

/// §13 the `?` operator (`ExprKind::Try`) over a finished operand. An `Ok`
/// unwraps to its inner value (`Ok(Ok(value))`); an `Err` is handed back
/// AS-IS for the caller to early-return from the enclosing function
/// (`Ok(Err(Value::Err(e)))` — the propagated value, NOT a fault); a
/// non-`Result` faults `GUARD_TYPE`. The interpreter's `KTry` and the emitted
/// `?` both call this, so the unwrap, the propagated value, and the fault
/// message + span cannot drift (CDR-006 §2). The control flow (push vs unwind
/// vs Rust `return`) stays per-engine; only the value decision is shared.
pub fn try_value(value: Value, span: Span) -> Result<Result<Value, Value>, RtError> {
    Ok(match value {
        Value::Ok(inner) => Ok((*inner).clone()),
        Value::Err(e) => Err(Value::Err(e)),
        other => {
            return Err(fault(
                codes::GUARD_TYPE,
                format!("`?` requires a `Result`, found `{}` (§13)", other.kind()),
                span,
            ));
        }
    })
}

/// §22 `filter` predicate guard — the per-element result must be `bool`
/// (keep on `true`). The SHARED check both engines call, so the
/// predicate-type fault is identical (CDR-006 §2).
pub fn filter_keep(result: &Value, span: Span) -> Result<bool, RtError> {
    match result {
        Value::Bool(b) => Ok(*b),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`filter` predicate must return `bool`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub fn cmp_guard(e: CmpError, span: Span) -> RtError {
    let msg = match e {
        CmpError::NotComparable(kind) => format!("`{kind}` values are not comparable"),
        CmpError::RecordShape => "records with different field sets are not comparable".into(),
        CmpError::Fuel => "comparison exceeded the structural budget (cyclic value?)".into(),
    };
    fault(codes::GUARD_COMPARE, msg, span)
}

pub(crate) fn arity_fault(expected: &str, found: usize, span: Span) -> RtError {
    fault(
        codes::GUARD_ARITY,
        format!("expected {expected} argument(s), found {found}"),
        span,
    )
}

/// Converts an unchecked runtime argument vector into the exact call shape a
/// leaf consumes. Arity validation and ownership transfer happen together, so
/// no program-reachable dispatcher needs to recover arguments with `unwrap`.
pub fn exact_args<const N: usize>(args: Vec<Value>, span: Span) -> Result<[Value; N], RtError> {
    let found = args.len();
    args.try_into()
        .map_err(|_| arity_fault(&N.to_string(), found, span))
}

pub(super) fn expect_bool(value: Value, context: &str, span: Span) -> Result<bool, RtError> {
    match value {
        Value::Bool(b) => Ok(b),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`{context}` takes a `bool`, found `{}`", other.kind()),
            span,
        )),
    }
}

pub(super) fn expect_string(value: Value, context: &str, span: Span) -> Result<Rc<str>, RtError> {
    match value {
        Value::Str(s) => Ok(s),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`{context}` takes a `string`, found `{}`", other.kind()),
            span,
        )),
    }
}
