use super::super::*;

pub(in crate::value) fn assert_fault(message: impl Into<String>, span: Span) -> RtError {
    fault(codes::FAULT_ASSERT, message, span)
}

pub fn builtin_test_assert(condition: Value, message: Value, span: Span) -> Result<Value, RtError> {
    let condition = expect_bool(condition, "Test.assert", span)?;
    let message = expect_string(message, "Test.assert message", span)?;
    if condition {
        Ok(Value::Unit)
    } else {
        Err(assert_fault(message.to_string(), span))
    }
}

pub fn builtin_test_assert_eq(
    actual: Value,
    expected: Value,
    span: Span,
) -> Result<Value, RtError> {
    if values_equal(&actual, &expected).map_err(|e| cmp_guard(e, span))? {
        Ok(Value::Unit)
    } else {
        Err(assert_fault(
            format!(
                "assertEq failed: actual `{}` != expected `{}`",
                render(&actual),
                render(&expected)
            ),
            span,
        ))
    }
}

pub fn builtin_test_assert_ne(
    actual: Value,
    expected: Value,
    span: Span,
) -> Result<Value, RtError> {
    if values_equal(&actual, &expected).map_err(|e| cmp_guard(e, span))? {
        Err(assert_fault(
            format!("assertNe failed: both values are `{}`", render(&actual)),
            span,
        ))
    } else {
        Ok(Value::Unit)
    }
}

pub fn builtin_test_assert_contains(
    text: Value,
    needle: Value,
    span: Span,
) -> Result<Value, RtError> {
    let text = expect_string(text, "Test.assertContains text", span)?;
    let needle = expect_string(needle, "Test.assertContains needle", span)?;
    if text.contains(needle.as_ref()) {
        Ok(Value::Unit)
    } else {
        Err(assert_fault(
            format!("assertContains failed: `{text}` does not contain `{needle}`"),
            span,
        ))
    }
}

pub fn builtin_test_assert_ok(value: Value, span: Span) -> Result<Value, RtError> {
    match value {
        Value::Ok(inner) => Ok((*inner).clone()),
        Value::Err(err) => Err(assert_fault(
            format!("assertOk failed: got Err({})", render(&err)),
            span,
        )),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`Test.assertOk` takes a `Result`, found `{}`", other.kind()),
            span,
        )),
    }
}

pub fn builtin_test_assert_err(value: Value, span: Span) -> Result<Value, RtError> {
    match value {
        Value::Err(err) => Ok((*err).clone()),
        Value::Ok(ok) => Err(assert_fault(
            format!("assertErr failed: got Ok({})", render(&ok)),
            span,
        )),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`Test.assertErr` takes a `Result`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub fn builtin_test_assert_some(value: Value, span: Span) -> Result<Value, RtError> {
    match value {
        Value::Some(inner) => Ok((*inner).clone()),
        Value::None => Err(assert_fault("assertSome failed: got None", span)),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`Test.assertSome` takes an `Option`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub fn builtin_test_assert_none(value: Value, span: Span) -> Result<Value, RtError> {
    match value {
        Value::None => Ok(Value::Unit),
        Value::Some(inner) => Err(assert_fault(
            format!("assertNone failed: got Some({})", render(&inner)),
            span,
        )),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`Test.assertNone` takes an `Option`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub fn builtin_test_assert_golden(
    host: &dyn Host,
    path: Value,
    actual: Value,
    span: Span,
) -> Result<Value, RtError> {
    let path = expect_string(path, "Test.assertGolden path", span)?;
    let actual = expect_string(actual, "Test.assertGolden actual", span)?;
    let handle = host.open(&path).map_err(|e| {
        assert_fault(
            format!("assertGolden failed: could not open `{path}`: {e}"),
            span,
        )
    })?;
    let expected = host.read(handle);
    host.close(handle);
    let expected = expected.map_err(|e| {
        assert_fault(
            format!("assertGolden failed: could not read `{path}`: {e}"),
            span,
        )
    })?;
    if expected == actual.as_ref() {
        Ok(Value::Unit)
    } else {
        Err(assert_fault(
            format!("assertGolden failed: `{path}` did not match actual output"),
            span,
        ))
    }
}

/// §18 shared assertion dispatcher. Interpreter and emitted runtime both call this
/// for first-class/default/named assertion builtins, so arity, default filling,
/// assertion failures, host golden reads, and unchecked type guards stay identical.
pub fn builtin_test_dispatch(
    host: &dyn Host,
    kind: Builtin,
    mut args: Vec<Value>,
    span: Span,
) -> Result<Value, RtError> {
    match kind {
        Builtin::TestAssert => {
            if !(1..=2).contains(&args.len()) {
                return Err(arity_fault("1 or 2", args.len(), span));
            }
            if args.len() == 1 {
                args.push(Value::str("assertion failed"));
            }
            let [condition, message] = exact_args(args, span)?;
            builtin_test_assert(condition, message, span)
        }
        Builtin::TestAssertEq => {
            let [actual, expected] = exact_args(args, span)?;
            builtin_test_assert_eq(actual, expected, span)
        }
        Builtin::TestAssertNe => {
            let [actual, expected] = exact_args(args, span)?;
            builtin_test_assert_ne(actual, expected, span)
        }
        Builtin::TestAssertContains => {
            let [text, needle] = exact_args(args, span)?;
            builtin_test_assert_contains(text, needle, span)
        }
        Builtin::TestAssertOk => {
            let [value] = exact_args(args, span)?;
            builtin_test_assert_ok(value, span)
        }
        Builtin::TestAssertErr => {
            let [value] = exact_args(args, span)?;
            builtin_test_assert_err(value, span)
        }
        Builtin::TestAssertSome => {
            let [value] = exact_args(args, span)?;
            builtin_test_assert_some(value, span)
        }
        Builtin::TestAssertNone => {
            let [value] = exact_args(args, span)?;
            builtin_test_assert_none(value, span)
        }
        Builtin::TestAssertGolden => {
            let [path, actual] = exact_args(args, span)?;
            builtin_test_assert_golden(host, path, actual, span)
        }
        _ => Err(fault(
            codes::GUARD_UNIMPLEMENTED,
            "the Test dispatcher received a non-Test builtin",
            span,
        )),
    }
}
