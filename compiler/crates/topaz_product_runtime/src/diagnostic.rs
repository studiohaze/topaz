use crate::*;

pub(crate) fn is_control_signal(error: &str) -> bool {
    matches!(
        error,
        PROPAGATE_SIGNAL | RETURN_SIGNAL | BREAK_SIGNAL | CONTINUE_SIGNAL
    )
}

pub(crate) type LocalFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

pub(crate) fn run_local<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => {}
        }
    }
}

pub(crate) struct YieldOnce(pub(crate) bool);

impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            Poll::Pending
        }
    }
}

pub(crate) fn runtime_diagnostic(error: RtError) -> String {
    let message = error
        .message
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!(
        "{RUNTIME_DIAGNOSTIC_PREFIX}{}\t{}\t{}\t{message}",
        error.code, error.span.lo, error.span.hi
    )
}

pub(crate) fn registered_runtime_code(code: &str) -> Option<&'static str> {
    use topaz_value::codes;
    Some(match code {
        "TPZ4001" => codes::FAULT_INDEX,
        "TPZ4002" => codes::FAULT_DIV_ZERO,
        "TPZ4003" => codes::FAULT_RANGE_STEP,
        "TPZ4004" => codes::FAULT_OVERFLOW,
        "TPZ4005" => codes::FAULT_NEG_EXPONENT,
        "TPZ4006" => codes::FAULT_MATCH_MISS,
        "TPZ4007" => codes::FAULT_ASSERT,
        "TPZ4601" => codes::FAULT_MAP_DUP_KEY,
        "TPZ5001" => codes::GUARD_TYPE,
        "TPZ5002" => codes::GUARD_UNBOUND,
        "TPZ5003" => codes::GUARD_IMMUTABLE,
        "TPZ5004" => codes::GUARD_ARITY,
        "TPZ5005" => codes::GUARD_NOT_CALLABLE,
        "TPZ5006" => codes::GUARD_NO_FIELD,
        "TPZ5007" => codes::GUARD_COMPARE,
        "TPZ5008" => codes::GUARD_REDECLARE,
        "TPZ5009" => codes::GUARD_RECURSION,
        "TPZ5099" => codes::GUARD_UNIMPLEMENTED,
        _ => return None,
    })
}

/// Recover a shared runtime diagnostic from an enriched self-product error.
pub fn decode_runtime_diagnostic(error: &str) -> Option<RtError> {
    let header = error
        .lines()
        .next()?
        .strip_prefix(RUNTIME_DIAGNOSTIC_PREFIX)?;
    let mut fields = header.splitn(4, '\t');
    let code = registered_runtime_code(fields.next()?)?;
    let lo = fields.next()?.parse().ok()?;
    let hi = fields.next()?.parse().ok()?;
    let encoded = fields.next()?;
    let mut message = String::with_capacity(encoded.len());
    let mut chars = encoded.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            message.push(character);
            continue;
        }
        match chars.next()? {
            '\\' => message.push('\\'),
            't' => message.push('\t'),
            'n' => message.push('\n'),
            'r' => message.push('\r'),
            _ => return None,
        }
    }
    Some(RtError {
        code,
        message,
        span: Span::new(FileId(0), lo, hi),
    })
}
