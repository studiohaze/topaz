use super::*;
use topaz_syntax::length::{LengthError, LengthScalar};
use topaz_syntax::{Length, LengthUnit};

/// The existing nominal carrier preserves U across boxed values, map keys,
/// closure/generic arguments and exported calls. A user newtype with the same
/// display id is not a compiler Length: declaration identity is also checked.
pub fn length_value(value: Length) -> Value {
    let id = value.unit.nominal();
    Value::newtype_with_identities(
        &id,
        format!("builtin:{id}"),
        Some(format!("builtin:{id}")),
        Value::Int(value.ticks),
    )
}
pub fn length_from_value(value: &Value) -> Option<Length> {
    let Value::Newtype {
        newtype_id,
        declaration_identity: Some(identity),
        method_identity: Some(method),
        inner,
        ..
    } = value
    else {
        return None;
    };
    let unit = LengthUnit::from_name(newtype_id.strip_prefix("Length<")?.strip_suffix('>')?)?;
    if identity.as_ref() != format!("builtin:{}", unit.nominal())
        || method.as_ref() != identity.as_ref()
    {
        return None;
    }
    let Value::Int(ticks) = inner.as_ref() else {
        return None;
    };
    Some(Length {
        unit,
        ticks: *ticks,
    })
}
pub fn length_fault(error: LengthError, span: Span) -> RtError {
    fault(
        if error == LengthError::DivisionByZero {
            codes::FAULT_DIV_ZERO
        } else if error == LengthError::Syntax {
            codes::GUARD_TYPE
        } else {
            codes::FAULT_OVERFLOW
        },
        error.message(),
        span,
    )
}
pub fn length_literal_value(text: &str, negative: bool, span: Span) -> Result<Value, RtError> {
    topaz_syntax::length::decode_length(text, negative)
        .map(length_value)
        .map_err(|e| length_fault(e, span))
}
fn scalar(value: &Value) -> Option<LengthScalar> {
    if let Some(length) = length_from_value(value) {
        return Some(LengthScalar::Length(length));
    }
    match value {
        Value::Int(n) => Some(LengthScalar::Int(*n)),
        Value::Float(n) => Some(LengthScalar::Float(*n)),
        _ => None,
    }
}
fn scalar_value(value: LengthScalar) -> Value {
    match value {
        LengthScalar::Length(v) => length_value(v),
        LengthScalar::Int(v) => Value::Int(v),
        LengthScalar::Float(v) => Value::Float(v),
        LengthScalar::Bool(v) => Value::Bool(v),
    }
}
pub fn length_binary_value(
    op: BinaryOp,
    left: &Value,
    right: &Value,
    span: Span,
) -> Result<Value, RtError> {
    let invalid = || {
        fault(
            codes::GUARD_TYPE,
            "Length operation requires compatible units and scalar operands",
            span,
        )
    };
    let (Some(a), Some(b)) = (scalar(left), scalar(right)) else {
        return Err(invalid());
    };
    topaz_syntax::length::length_binary(op, a, b)
        .ok_or_else(invalid)?
        .map(scalar_value)
        .map_err(|e| length_fault(e, span))
}
pub fn length_method(
    recv: Length,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RtError> {
    if recv.unit == LengthUnit::Em && method == "resolve" {
        let [font] = exact_args(args, span)?;
        let font = length_from_value(&font)
            .filter(|f| f.unit == LengthUnit::Pt)
            .ok_or_else(|| {
                fault(
                    codes::GUARD_TYPE,
                    "Length.resolve requires Length<Pt> fontSize",
                    span,
                )
            })?;
        return recv
            .resolve(font)
            .map(length_value)
            .map_err(|e| length_fault(e, span));
    }
    let [] = exact_args(args, span)?;
    if recv.unit != LengthUnit::Em {
        let target = match method {
            "toPt" => Some(LengthUnit::Pt),
            "toMm" => Some(LengthUnit::Mm),
            "toCm" => Some(LengthUnit::Cm),
            "toIn" => Some(LengthUnit::In),
            _ => None,
        };
        if let Some(unit) = target {
            return Ok(length_value(recv.convert(unit)));
        }
        if method == "scaledPoints" {
            return Ok(Value::Int(recv.ticks));
        }
        if method == "toMilliPoints" && recv.unit == LengthUnit::Pt {
            return recv
                .milli_points()
                .map(Value::Int)
                .map_err(|e| length_fault(e, span));
        }
    }
    Err(fault(
        codes::GUARD_TYPE,
        format!("{} has no member `{method}`", recv.unit.nominal()),
        span,
    ))
}
pub fn length_constructor(kind: Builtin, value: Value, span: Span) -> Result<Value, RtError> {
    let Value::Int(ticks) = value else {
        return Err(fault(
            codes::GUARD_TYPE,
            "Length constructor requires int",
            span,
        ));
    };
    let value = match kind {
        Builtin::LengthFromScaledPoints => Length {
            unit: LengthUnit::Pt,
            ticks,
        },
        Builtin::LengthFromMilliPoints => {
            Length::from_milli_points(ticks).map_err(|e| length_fault(e, span))?
        }
        _ => unreachable!("Length constructor tag"),
    };
    Ok(length_value(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn span() -> Span {
        Span::new(topaz_diag::FileId(7), 11, 33)
    }
    #[test]
    fn nominal_grid_identity_and_existing_canonical_keys() {
        let pt = length_value(Length {
            unit: LengthUnit::Pt,
            ticks: 65536,
        });
        let mm = length_value(Length {
            unit: LengthUnit::Mm,
            ticks: 65536,
        });
        let user = Value::newtype("Length<Pt>", Value::Int(65536));
        assert_eq!(render(&pt), "Length<Pt>(65536)");
        assert!(!values_equal(&pt, &mm).unwrap());
        assert!(!values_equal(&pt, &user).unwrap());
        let key = canonical_key(&pt).unwrap();
        let copy = key_to_value(&key);
        assert_eq!(
            length_from_value(&copy),
            Some(Length {
                unit: LengthUnit::Pt,
                ticks: 65536
            })
        );
        assert!(length_from_value(&user).is_none());
        assert_eq!(
            binary_value(BinaryOp::Add, pt.clone(), pt.clone(), span())
                .map(|v| render(&v))
                .unwrap(),
            "Length<Pt>(131072)"
        );
        assert!(binary_value(BinaryOp::Eq, pt.clone(), mm, span()).is_err());
        assert!(call_method(pt.clone(), "value", vec![], span(), span()).is_err());
        assert!(
            newtype_value_with_identity(
                pt.clone(),
                "Length<Pt>",
                Some("builtin:Length<Pt>"),
                span()
            )
            .is_err()
        );
        assert!(
            json_stringify(&pt, true)
                .unwrap_err()
                .starts_with("JSON_UNSUPPORTED:")
        );
        assert!(
            canonical_abi_encode(&pt)
                .unwrap_err()
                .starts_with("ABI_UNSUPPORTED:")
        );
        let user = Value::newtype("UserId", Value::Int(7));
        assert_eq!(json_stringify(&user, true).unwrap(), "7");
        assert_eq!(
            render(&call_method(user, "value", vec![], span(), span()).unwrap()),
            "7"
        );
    }
    #[test]
    fn shared_leaf_fault_code_message_and_original_span() {
        let em = length_value(Length {
            unit: LengthUnit::Em,
            ticks: 65536,
        });
        for ticks in [0, -1] {
            let font = length_value(Length {
                unit: LengthUnit::Pt,
                ticks,
            });
            let error = call_method(em.clone(), "resolve", vec![font], span(), span()).unwrap_err();
            assert_eq!(error.code, codes::FAULT_OVERFLOW);
            assert_eq!(error.message, "Length.resolve font size must be positive");
            assert_eq!(error.span, span());
        }
        assert!(receiver_builtin(&em, "scaledPoints").is_none());
        assert!(receiver_builtin(&em, "value").is_none());
    }
}
