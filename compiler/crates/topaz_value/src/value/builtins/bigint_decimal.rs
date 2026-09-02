use super::super::*;

pub(in crate::value) fn bigint_arg(
    arg: Value,
    owner: &str,
    name: &str,
    span: Span,
) -> Result<Rc<BigIntData>, RtError> {
    match arg {
        Value::BigInt(n) => Ok(n),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`{owner}.{name}` takes `BigInt`, found `{}`", other.kind()),
            span,
        )),
    }
}

pub(in crate::value) fn bigint_int_arg(
    arg: Value,
    owner: &str,
    name: &str,
    param: &str,
    span: Span,
) -> Result<i64, RtError> {
    match arg {
        Value::Int(n) => Ok(n),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`{owner}.{name}` takes `{param}: int`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub fn builtin_bigint_from_int(n: Value, span: Span) -> Result<Value, RtError> {
    let n = bigint_int_arg(n, "BigInt", "fromInt", "n", span)?;
    Ok(Value::BigInt(Rc::new(BigIntData::from_i64(n))))
}

pub fn builtin_bigint_parse(text: Value, radix: Value, span: Span) -> Result<Value, RtError> {
    let text = stdlib_string_arg(text, "BigInt", "parse", "text", span)?;
    let radix = bigint_int_arg(radix, "BigInt", "parse", "radix", span)?;
    Ok(match BigIntData::parse_radix(text.trim(), radix) {
        Some(n) => Value::Some(Rc::new(Value::BigInt(Rc::new(n)))),
        None => Value::None,
    })
}

pub fn builtin_bigint_to_string(recv: Value, radix: Value, span: Span) -> Result<Value, RtError> {
    let n = bigint_arg(recv, "BigInt", "toString", span)?;
    let radix = bigint_int_arg(radix, "BigInt", "toString", "radix", span)?;
    if !(2..=36).contains(&radix) {
        return Err(fault(
            codes::GUARD_TYPE,
            "`BigInt.toString` radix must be between 2 and 36",
            span,
        ));
    }
    Ok(Value::Str(Rc::from(n.to_string_radix(radix as u32))))
}

pub fn builtin_bigint_to_int(recv: Value, span: Span) -> Result<Value, RtError> {
    let n = bigint_arg(recv, "BigInt", "toInt", span)?;
    Ok(match n.to_i64() {
        Some(v) => Value::Some(Rc::new(Value::Int(v))),
        None => Value::None,
    })
}

pub fn builtin_bigint_div(recv: Value, other: Value, span: Span) -> Result<Value, RtError> {
    let n = bigint_arg(recv, "BigInt", "div", span)?;
    let d = bigint_arg(other, "BigInt", "div", span)?;
    Ok(match n.div_mod(&d) {
        Some((q, _)) => Value::Ok(Rc::new(Value::BigInt(Rc::new(q)))),
        None => Value::Err(Rc::new(Value::str("BigInt.div: division by zero"))),
    })
}

pub fn builtin_bigint_mod(recv: Value, other: Value, span: Span) -> Result<Value, RtError> {
    let n = bigint_arg(recv, "BigInt", "mod", span)?;
    let d = bigint_arg(other, "BigInt", "mod", span)?;
    Ok(match n.div_mod(&d) {
        Some((_, r)) => Value::Ok(Rc::new(Value::BigInt(Rc::new(r)))),
        None => Value::Err(Rc::new(Value::str("BigInt.mod: division by zero"))),
    })
}

pub(in crate::value) fn decimal_arg(
    arg: Value,
    owner: &str,
    name: &str,
    span: Span,
) -> Result<Rc<DecimalData>, RtError> {
    match arg {
        Value::Decimal(d) => Ok(d),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`{owner}.{name}` takes `Decimal`, found `{}`", other.kind()),
            span,
        )),
    }
}

pub(in crate::value) fn decimal_scale_arg(
    scale: Value,
    owner: &str,
    name: &str,
    span: Span,
) -> Result<u32, RtError> {
    match scale {
        Value::Int(n) if n >= 0 => n.try_into().map_err(|_| {
            fault(
                codes::GUARD_TYPE,
                format!("`{owner}.{name}` scale must fit a non-negative 32-bit integer"),
                span,
            )
        }),
        Value::Int(_) => Err(fault(
            codes::GUARD_TYPE,
            format!("`{owner}.{name}` scale must be non-negative"),
            span,
        )),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`{owner}.{name}` scale must be `int`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub(in crate::value) fn rounding_mode_arg(
    mode: Value,
    owner: &str,
    name: &str,
    span: Span,
) -> Result<RoundingMode, RtError> {
    match mode {
        Value::Enum {
            enum_id,
            variant,
            payloads,
            ..
        } if enum_id.as_ref() == "RoundingMode" && payloads.is_empty() => {
            RoundingMode::from_name(&variant).ok_or_else(|| {
                fault(
                    codes::GUARD_TYPE,
                    format!("`{owner}.{name}` got unknown RoundingMode `{variant}`"),
                    span,
                )
            })
        }
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`{owner}.{name}` mode must be `RoundingMode`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub fn builtin_decimal_from_int(n: Value, span: Span) -> Result<Value, RtError> {
    let n = bigint_int_arg(n, "Decimal", "fromInt", "n", span)?;
    Ok(Value::Decimal(Rc::new(DecimalData::from_i64(n))))
}

pub fn builtin_decimal_parse(text: Value, span: Span) -> Result<Value, RtError> {
    let text = stdlib_string_arg(text, "Decimal", "parse", "text", span)?;
    Ok(match DecimalData::parse(&text) {
        Some(d) => Value::Some(Rc::new(Value::Decimal(Rc::new(d)))),
        None => Value::None,
    })
}

pub fn builtin_decimal_to_string(recv: Value, span: Span) -> Result<Value, RtError> {
    let d = decimal_arg(recv, "Decimal", "toString", span)?;
    Ok(Value::Str(Rc::from(d.to_string_canonical())))
}

pub fn builtin_decimal_scale(recv: Value, span: Span) -> Result<Value, RtError> {
    let d = decimal_arg(recv, "Decimal", "scale", span)?;
    Ok(Value::Int(d.scale as i64))
}

pub fn builtin_decimal_to_int(recv: Value, span: Span) -> Result<Value, RtError> {
    let d = decimal_arg(recv, "Decimal", "toInt", span)?;
    Ok(match d.to_i64() {
        Some(v) => Value::Some(Rc::new(Value::Int(v))),
        None => Value::None,
    })
}

pub fn builtin_decimal_round(
    recv: Value,
    scale: Value,
    mode: Value,
    span: Span,
) -> Result<Value, RtError> {
    let d = decimal_arg(recv, "Decimal", "round", span)?;
    let scale = decimal_scale_arg(scale, "Decimal", "round", span)?;
    let mode = rounding_mode_arg(mode, "Decimal", "round", span)?;
    Ok(Value::Decimal(Rc::new(d.round_to_scale(scale, mode))))
}

pub fn builtin_decimal_div(
    recv: Value,
    other: Value,
    scale: Value,
    mode: Value,
    span: Span,
) -> Result<Value, RtError> {
    let d = decimal_arg(recv, "Decimal", "div", span)?;
    let other = decimal_arg(other, "Decimal", "div", span)?;
    let scale = decimal_scale_arg(scale, "Decimal", "div", span)?;
    let mode = rounding_mode_arg(mode, "Decimal", "div", span)?;
    Ok(match d.div_rounded(&other, scale, mode) {
        Some(out) => Value::Ok(Rc::new(Value::Decimal(Rc::new(out)))),
        None => Value::Err(Rc::new(Value::str("Decimal.div: division by zero"))),
    })
}
