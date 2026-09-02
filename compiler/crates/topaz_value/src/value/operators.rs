use super::*;

// --- Scalar checked-arithmetic leaf (CDR-006 §2 shared-leaf discipline) ---
//
// The SINGLE source of truth for scalar `int`/`float` operation semantics:
// `binary_value`/`unary_value` call these, and the v5.4 native backend will
// call the SAME helpers, so monomorphized native arithmetic stays BYTE-IDENTICAL
// to the interpreter (checked-int overflow TPZ4004, div/rem-by-0 TPZ4002,
// `i64::MIN / -1`, negative-exponent TPZ4005 — exact codes, messages, and spans).
// Each is `#[inline]` so the boxed-dispatch caller pays nothing and the native
// caller inlines the bare scalar fault into its generated code.

/// `a + b` with the §13a checked-overflow fault (TPZ4004).
#[inline]
pub fn int_add(a: i64, b: i64, span: Span) -> Result<i64, RtError> {
    a.checked_add(b)
        .ok_or_else(|| fault(codes::FAULT_OVERFLOW, "integer addition overflows", span))
}

/// `a - b` with the §13a checked-overflow fault (TPZ4004).
#[inline]
pub fn int_sub(a: i64, b: i64, span: Span) -> Result<i64, RtError> {
    a.checked_sub(b)
        .ok_or_else(|| fault(codes::FAULT_OVERFLOW, "integer subtraction overflows", span))
}

/// `a * b` with the §13a checked-overflow fault (TPZ4004).
#[inline]
pub fn int_mul(a: i64, b: i64, span: Span) -> Result<i64, RtError> {
    a.checked_mul(b).ok_or_else(|| {
        fault(
            codes::FAULT_OVERFLOW,
            "integer multiplication overflows",
            span,
        )
    })
}

/// `a / b`: div-by-zero faults TPZ4002 (`integer division by zero`); the
/// `i64::MIN / -1` overflow faults TPZ4004 (`checked_div` returns `None`).
#[inline]
pub fn int_div(a: i64, b: i64, span: Span) -> Result<i64, RtError> {
    if b == 0 {
        return Err(fault(
            codes::FAULT_DIV_ZERO,
            "integer division by zero",
            span,
        ));
    }
    a.checked_div(b)
        .ok_or_else(|| fault(codes::FAULT_OVERFLOW, "integer division overflows", span))
}

/// `a % b`: rem-by-zero faults TPZ4002 (`integer remainder by zero`); the
/// `i64::MIN % -1` overflow faults TPZ4004 (`checked_rem` returns `None`).
#[inline]
pub fn int_rem(a: i64, b: i64, span: Span) -> Result<i64, RtError> {
    if b == 0 {
        return Err(fault(
            codes::FAULT_DIV_ZERO,
            "integer remainder by zero",
            span,
        ));
    }
    a.checked_rem(b)
        .ok_or_else(|| fault(codes::FAULT_OVERFLOW, "integer remainder overflows", span))
}

/// `a ** b`: a negative exponent faults TPZ4005; an overflowing (or
/// out-of-`u32`) exponent faults TPZ4004 — all `integer exponentiation
/// overflows` except the negative case.
#[inline]
pub fn int_pow(a: i64, b: i64, span: Span) -> Result<i64, RtError> {
    if b < 0 {
        return Err(fault(
            codes::FAULT_NEG_EXPONENT,
            "integer exponent must be non-negative; use float operands",
            span,
        ));
    }
    let exp: u32 = b.try_into().map_err(|_| {
        fault(
            codes::FAULT_OVERFLOW,
            "integer exponentiation overflows",
            span,
        )
    })?;
    a.checked_pow(exp).ok_or_else(|| {
        fault(
            codes::FAULT_OVERFLOW,
            "integer exponentiation overflows",
            span,
        )
    })
}

/// `-x` with the §13a checked-negation fault (TPZ4004, `i64::MIN`).
#[inline]
pub fn int_neg(x: i64, span: Span) -> Result<i64, RtError> {
    x.checked_neg()
        .ok_or_else(|| fault(codes::FAULT_OVERFLOW, "integer negation overflows", span))
}

/// Integer comparison (`<`, `<=`, `>`, `>=`). Total — never faults.
#[inline]
pub fn int_cmp(op: BinaryOp, a: i64, b: i64) -> bool {
    match op {
        BinaryOp::Lt => a < b,
        BinaryOp::Le => a <= b,
        BinaryOp::Gt => a > b,
        BinaryOp::Ge => a >= b,
        _ => unreachable!("int_cmp called with a non-comparison operator"),
    }
}

/// Canonical quiet NaN emitted by Topaz arithmetic operations.
///
/// IEEE-754 leaves the sign and payload of a newly produced NaN unspecified.
/// Normalizing only arithmetic results keeps trace bytes stable across host
/// architectures without changing the bits of NaNs received from literals or
/// external values until an arithmetic operation consumes them.
pub const CANONICAL_ARITHMETIC_NAN_BITS: u64 = 0x7ff8_0000_0000_0000;

#[inline]
pub(super) fn canonicalize_arithmetic_nan(value: f64) -> f64 {
    if value.is_nan() {
        f64::from_bits(CANONICAL_ARITHMETIC_NAN_BITS)
    } else {
        value
    }
}

/// Float arithmetic (`+`, `-`, `*`, `/`, `**`): IEEE-754 value semantics,
/// never a fault (`/0.0` is `inf`/`NaN`). Newly produced NaNs use one pinned
/// quiet-NaN bit pattern so backend traces do not inherit host-specific NaN
/// sign or payload choices. `%` is `int`-only and is NOT routed here — the
/// caller raises its own GUARD_TYPE.
#[inline]
pub fn float_arith(op: BinaryOp, a: f64, b: f64) -> f64 {
    let value = match op {
        BinaryOp::Add => a + b,
        BinaryOp::Sub => a - b,
        BinaryOp::Mul => a * b,
        BinaryOp::Div => a / b, // IEEE: inf/NaN
        BinaryOp::Pow => a.powf(b),
        _ => unreachable!("float_arith called with a non-arithmetic operator"),
    };
    canonicalize_arithmetic_nan(value)
}

/// Float comparison (`<`, `<=`, `>`, `>=`): IEEE-754 ordering. Total — never
/// faults (equality/inequality go through `values_equal`).
#[inline]
pub fn float_cmp(op: BinaryOp, a: f64, b: f64) -> bool {
    match op {
        BinaryOp::Lt => a < b,
        BinaryOp::Le => a <= b,
        BinaryOp::Gt => a > b,
        BinaryOp::Ge => a >= b,
        _ => unreachable!("float_cmp called with a non-comparison operator"),
    }
}

/// §2 unary operator semantics over a finished operand.
pub fn unary_value(op: UnaryOp, v: Value, span: Span) -> Result<Value, RtError> {
    Ok(match (op, v) {
        (UnaryOp::Plus, Value::Int(x)) => Value::Int(x),
        (UnaryOp::Plus, Value::Float(x)) => Value::Float(x),
        (UnaryOp::Plus, Value::BigInt(x)) => Value::BigInt(x),
        (UnaryOp::Plus, Value::Decimal(x)) => Value::Decimal(x),
        (UnaryOp::Minus, Value::Int(x)) => Value::Int(int_neg(x, span)?),
        (UnaryOp::Minus, Value::Float(x)) => Value::Float(-x),
        (UnaryOp::Minus, Value::BigInt(x)) => Value::BigInt(Rc::new(x.neg())),
        (UnaryOp::Minus, Value::Decimal(x)) => Value::Decimal(Rc::new(x.neg())),
        (UnaryOp::Not, Value::Bool(b)) => Value::Bool(!b),
        (op, v) => {
            return Err(fault(
                codes::GUARD_TYPE,
                format!("unary `{op:?}` is not defined for `{}`", v.kind()),
                span,
            ));
        }
    })
}

/// §2 binary operator semantics over finished operands.
pub fn binary_value(op: BinaryOp, lhs: Value, rhs: Value, span: Span) -> Result<Value, RtError> {
    use BinaryOp::*;
    use Value as V;
    Ok(match (op, lhs, rhs) {
        // Scalar integer arithmetic routes through the shared checked leaf
        // (`int_add`/…), the SINGLE source of truth the native backend reuses.
        (Add, V::Int(a), V::Int(b)) => V::Int(int_add(a, b, span)?),
        (Sub, V::Int(a), V::Int(b)) => V::Int(int_sub(a, b, span)?),
        (Mul, V::Int(a), V::Int(b)) => V::Int(int_mul(a, b, span)?),
        (Div, V::Int(a), V::Int(b)) => V::Int(int_div(a, b, span)?),
        (Rem, V::Int(a), V::Int(b)) => V::Int(int_rem(a, b, span)?),
        (Pow, V::Int(a), V::Int(b)) => V::Int(int_pow(a, b, span)?),
        (Add, V::BigInt(a), V::BigInt(b)) => V::BigInt(Rc::new(a.add(&b))),
        (Sub, V::BigInt(a), V::BigInt(b)) => V::BigInt(Rc::new(a.sub(&b))),
        (Mul, V::BigInt(a), V::BigInt(b)) => V::BigInt(Rc::new(a.mul(&b))),
        (Div | Rem | Pow, V::BigInt(_), V::BigInt(_)) => {
            return Err(fault(
                codes::GUARD_TYPE,
                "`BigInt` division and remainder use `.div()` / `.mod()` so divide-by-zero is a value-level error",
                span,
            ));
        }
        (Add, V::Decimal(a), V::Decimal(b)) => V::Decimal(Rc::new(a.add(&b))),
        (Sub, V::Decimal(a), V::Decimal(b)) => V::Decimal(Rc::new(a.sub(&b))),
        (Mul, V::Decimal(a), V::Decimal(b)) => V::Decimal(Rc::new(a.mul(&b).ok_or_else(|| {
            fault(
                codes::FAULT_OVERFLOW,
                "Decimal multiplication scale overflows",
                span,
            )
        })?)),
        (Div | Rem | Pow, V::Decimal(_), V::Decimal(_)) => {
            return Err(fault(
                codes::GUARD_TYPE,
                "`Decimal` division, remainder, and exponentiation land in a later decimal slice",
                span,
            ));
        }
        // Scalar float arithmetic routes through the shared IEEE leaf.
        (Add | Sub | Mul | Div | Pow, V::Float(a), V::Float(b)) => V::Float(float_arith(op, a, b)),
        (Rem, V::Float(_), V::Float(_)) => {
            return Err(fault(
                codes::GUARD_TYPE,
                "`%` is `int`-only; floating-point remainder is a standard-library matter (§2)",
                span,
            ));
        }
        (Add, V::Str(a), V::Str(b)) => {
            let mut s = String::with_capacity(a.len() + b.len());
            s.push_str(&a);
            s.push_str(&b);
            Value::str(s)
        }
        (Eq, a, b) => V::Bool(values_equal(&a, &b).map_err(|e| cmp_guard(e, span))?),
        (Ne, a, b) => V::Bool(!values_equal(&a, &b).map_err(|e| cmp_guard(e, span))?),
        (Lt | Le | Gt | Ge, V::Int(a), V::Int(b)) => V::Bool(int_cmp(op, a, b)),
        (Lt | Le | Gt | Ge, V::Float(a), V::Float(b)) => V::Bool(float_cmp(op, a, b)),
        (Lt, V::Str(a), V::Str(b)) => V::Bool(a < b),
        (Le, V::Str(a), V::Str(b)) => V::Bool(a <= b),
        (Gt, V::Str(a), V::Str(b)) => V::Bool(a > b),
        (Ge, V::Str(a), V::Str(b)) => V::Bool(a >= b),
        // §8 (v5.4) `Bytes` order LEXICOGRAPHICALLY by byte (`[u8]` cmp, like `string`).
        // The checker admits `<`/`<=`/`>`/`>=` over two `Bytes` (order_comparable_gate),
        // so these arms keep check==runtime — a Bytes order op never falls to the
        // GUARD_TYPE catch-all below.
        (Lt, V::Bytes(a), V::Bytes(b)) => V::Bool(a < b),
        (Le, V::Bytes(a), V::Bytes(b)) => V::Bool(a <= b),
        (Gt, V::Bytes(a), V::Bytes(b)) => V::Bool(a > b),
        (Ge, V::Bytes(a), V::Bytes(b)) => V::Bool(a >= b),
        (Lt, V::Url(a), V::Url(b)) => V::Bool(a.canonical.as_ref() < b.canonical.as_ref()),
        (Le, V::Url(a), V::Url(b)) => V::Bool(a.canonical.as_ref() <= b.canonical.as_ref()),
        (Gt, V::Url(a), V::Url(b)) => V::Bool(a.canonical.as_ref() > b.canonical.as_ref()),
        (Ge, V::Url(a), V::Url(b)) => V::Bool(a.canonical.as_ref() >= b.canonical.as_ref()),
        (Lt, V::BigInt(a), V::BigInt(b)) => V::Bool(a < b),
        (Le, V::BigInt(a), V::BigInt(b)) => V::Bool(a <= b),
        (Gt, V::BigInt(a), V::BigInt(b)) => V::Bool(a > b),
        (Ge, V::BigInt(a), V::BigInt(b)) => V::Bool(a >= b),
        (Lt, V::Decimal(a), V::Decimal(b)) => V::Bool(a < b),
        (Le, V::Decimal(a), V::Decimal(b)) => V::Bool(a <= b),
        (Gt, V::Decimal(a), V::Decimal(b)) => V::Bool(a > b),
        (Ge, V::Decimal(a), V::Decimal(b)) => V::Bool(a >= b),
        // §13 (v5.4) `Date` orders by its Gregorian day count.
        (Lt, V::Date(a), V::Date(b)) => V::Bool(a < b),
        (Le, V::Date(a), V::Date(b)) => V::Bool(a <= b),
        (Gt, V::Date(a), V::Date(b)) => V::Bool(a > b),
        (Ge, V::Date(a), V::Date(b)) => V::Bool(a >= b),
        // §3 (v5.4) ORDERING over NOMINAL values (record/enum/newtype): route through
        // the SHARED `values_compare` leaf so interp ≡ boxed emit by construction. The
        // checker only admits `<` between two order-comparable nominals of the SAME
        // type, so the leaf returns an `Ordering` (a different nominal id / non-orderable
        // inner faults GUARD_COMPARE, reachable only `--unchecked`). The operator maps
        // the ordering to a bool the SAME way the int arm does.
        (
            Lt | Le | Gt | Ge,
            a @ (V::NominalRecord { .. } | V::Enum { .. } | V::Newtype { .. }),
            b,
        )
        | (
            Lt | Le | Gt | Ge,
            a,
            b @ (V::NominalRecord { .. } | V::Enum { .. } | V::Newtype { .. }),
        ) => {
            let ord = values_compare(&a, &b).map_err(|e| cmp_guard(e, span))?;
            use std::cmp::Ordering;
            V::Bool(match op {
                Lt => ord == Ordering::Less,
                Le => ord != Ordering::Greater,
                Gt => ord == Ordering::Greater,
                Ge => ord != Ordering::Less,
                _ => unreachable!(),
            })
        }
        (In, needle, V::Array(items)) => {
            let items = items.borrow();
            let mut found = false;
            for item in items.iter() {
                if values_equal(&needle, item).map_err(|e| cmp_guard(e, span))? {
                    found = true;
                    break;
                }
            }
            V::Bool(found)
        }
        (In, needle, V::Set(set)) => V::Bool(
            set.borrow()
                .contains_value(&needle)
                .map_err(|e| cmp_guard(e, span))?,
        ),
        (
            In,
            V::Int(v),
            V::Range {
                lo,
                hi,
                inclusive,
                step,
            },
        ) => {
            if step == 0 {
                return Err(fault(
                    codes::FAULT_RANGE_STEP,
                    "range step must not be zero (§10)",
                    span,
                ));
            }
            let within = if step > 0 {
                v >= lo && if inclusive { v <= hi } else { v < hi }
            } else {
                v <= lo && if inclusive { v >= hi } else { v > hi }
            };
            // §10 the stepping check `(v - lo) % step == 0` — computed in i128 so a
            // huge span (`v - lo` up to `i64::MAX - i64::MIN`, which overflows i64)
            // does not panic (debug) / wrap to a wrong result (release). The bounds
            // check above is comparison-only, so it cannot overflow.
            V::Bool(within && (v as i128 - lo as i128) % step as i128 == 0)
        }
        (In, _, V::Map(_)) => {
            return Err(fault(
                codes::GUARD_TYPE,
                "`x in map` is a static error; use `x in map.keys` (§9)",
                span,
            ));
        }
        (op, a, b) => {
            return Err(fault(
                codes::GUARD_TYPE,
                format!(
                    "`{op:?}` is not defined for `{}` and `{}`",
                    a.kind(),
                    b.kind()
                ),
                span,
            ));
        }
    })
}

/// §10 stepped materialization for `for` over integer ranges.
pub fn range_items(range: &Value, span: Span) -> Result<Vec<Value>, RtError> {
    let Value::Range {
        lo,
        hi,
        inclusive,
        step,
    } = range
    else {
        unreachable!()
    };
    if *step == 0 {
        return Err(fault(
            codes::FAULT_RANGE_STEP,
            "range step must not be zero (§10)",
            span,
        ));
    }
    let mut out = Vec::new();
    let mut v = *lo;
    loop {
        let within = if *step > 0 {
            if *inclusive { v <= *hi } else { v < *hi }
        } else if *inclusive {
            v >= *hi
        } else {
            v > *hi
        };
        if !within {
            break;
        }
        out.push(Value::Int(v));
        v = match v.checked_add(*step) {
            Some(next) => next,
            None => break,
        };
    }
    Ok(out)
}

/// §12 wrap a member result back into the optional container after an
/// `obj?.x` on a `Some` — ONE layer (an already-optional result is not
/// re-wrapped). Shared so both engines agree.
pub fn wrap_optional(v: Value) -> Value {
    match v {
        Value::Some(_) | Value::None => v,
        other => Value::Some(Rc::new(other)),
    }
}

/// §12 optional member access `object?.field`: `None`/`null` short-circuit
/// (preserved); a `Some(inner)` unwraps, accesses `inner.field`, and re-wraps
/// via [`wrap_optional`]; any other value accesses `field` directly. The member
/// access goes through [`member_value_required`], including access-only
/// properties and read-only bound-method values that the boxed emitter can call
/// through the shared runtime bridge.
pub fn optional_member(object: Value, field: &str, span: Span) -> Result<Value, RtError> {
    match object {
        Value::None => Ok(Value::None),
        Value::Null => Ok(Value::Null),
        Value::Some(inner) => Ok(wrap_optional(member_value_required(
            inner.as_ref(),
            field,
            span,
        )?)),
        other => member_value_required(&other, field, span),
    }
}

/// §8 record update `{ ...base, field: value }` — STAGE 1: the base must be a
/// record (a non-record faults GUARD_TYPE). Split from the merge so the base
/// is type-checked BEFORE the field values are evaluated, exactly as the
/// interpreter (`KRecordUpdateBase` runs before `KRecord`).
pub fn record_update_base(base: Value, span: Span) -> Result<Rc<BTreeMap<String, Value>>, RtError> {
    match base {
        Value::Record(map) => Ok(map),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("record update needs a record, found `{}`", other.kind()),
            span,
        )),
    }
}

/// §3 (v5.4) NOMINAL spread-update `User { ...base, … }` — validate that `base`
/// is a `NominalRecord` of `record_id` and return its decl-ordered fields. A
/// wrong-id / non-record base faults GUARD_TYPE. The interpreter (`KNominalSpread`)
/// and the boxed emitter both go through this leaf so the validation + fault
/// message are byte-identical (run≡build, and `--unchecked` consistency).
pub fn nominal_spread_base(
    base: Value,
    record_id: &str,
    span: Span,
) -> Result<Vec<(Rc<str>, Value)>, RtError> {
    nominal_spread_base_with_identity(base, record_id, None, span)
}

pub fn nominal_spread_base_with_identity(
    base: Value,
    record_id: &str,
    declaration_identity: Option<&str>,
    span: Span,
) -> Result<Vec<(Rc<str>, Value)>, RtError> {
    match base {
        Value::NominalRecord {
            record_id: base_id,
            declaration_identity: base_identity,
            fields,
            ..
        } if nominal_declaration_identity(&base_id, base_identity.as_deref())
            == nominal_declaration_identity(record_id, declaration_identity) =>
        {
            Ok(fields.to_vec())
        }
        Value::NominalRecord {
            record_id: base_id, ..
        } => Err(fault(
            codes::GUARD_TYPE,
            format!("record spread `...` needs a `{record_id}`, found a `{base_id}`"),
            span,
        )),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "record spread `...` needs a `{record_id}`, found a {}",
                other.kind()
            ),
            span,
        )),
    }
}

/// Validate and canonicalize a nominal spread base against the declaring record's
/// field inventory. Export adapters accept typed [`Value`] arguments directly, so
/// a host can supply the right nominal identity with an incomplete field vector.
/// Such a value is a runtime guard fault, not permission for either engine to
/// assume a later lookup succeeds. The returned seed follows `required_fields`
/// order after rejecting unknown, duplicate, or missing host fields with the same
/// guards ordinary nominal construction uses.
pub fn nominal_spread_base_required(
    base: Value,
    record_id: &str,
    declaration_identity: Option<&str>,
    required_fields: &[&str],
    span: Span,
) -> Result<Vec<(Rc<str>, Value)>, RtError> {
    let fields = nominal_spread_base_with_identity(base, record_id, declaration_identity, span)?;
    let mut seen = Vec::with_capacity(fields.len());
    for (field, _) in &fields {
        if !required_fields.contains(&field.as_ref()) {
            return Err(fault(
                codes::GUARD_NO_FIELD,
                format!("record `{record_id}` has no field `{field}`"),
                span,
            ));
        }
        if seen.contains(&field.as_ref()) {
            return Err(fault(
                codes::GUARD_ARITY,
                format!("field `{field}` is given twice in `{record_id}`"),
                span,
            ));
        }
        seen.push(field.as_ref());
    }
    required_fields
        .iter()
        .map(|field| {
            nominal_record_field_required(&fields, record_id, field, span)
                .map(|value| (Rc::from(*field), value))
        })
        .collect()
}

/// Read one required field from a nominal record accumulator. The reverse lookup
/// preserves explicit-field-over-spread precedence in the interpreter while also
/// giving generated Rust the same structured fault for an incomplete host value.
pub fn nominal_record_field_required(
    fields: &[(Rc<str>, Value)],
    record_id: &str,
    field: &str,
    span: Span,
) -> Result<Value, RtError> {
    fields
        .iter()
        .rev()
        .find(|(name, _)| name.as_ref() == field)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| {
            fault(
                codes::GUARD_ARITY,
                format!("record `{record_id}` is missing field `{field}`"),
                span,
            )
        })
}

/// §3 (v5.4) NEWTYPE unwrap `id.value()` — validate that `recv` is a `Newtype` of
/// `newtype_id` and return its wrapped inner value. A wrong-id / non-newtype
/// receiver faults GUARD_TYPE. The interpreter and the boxed emitter both go
/// through this leaf so the unwrapped value AND the fault message are
/// byte-identical (run≡build, and `--unchecked` consistency — no rustc leak).
pub fn newtype_value(recv: Value, newtype_id: &str, span: Span) -> Result<Value, RtError> {
    newtype_value_with_identity(recv, newtype_id, None, span)
}

pub fn newtype_value_with_identity(
    recv: Value,
    newtype_id: &str,
    declaration_identity: Option<&str>,
    span: Span,
) -> Result<Value, RtError> {
    match recv {
        Value::Newtype {
            newtype_id: id,
            declaration_identity: identity,
            inner,
            ..
        } if nominal_declaration_identity(&id, identity.as_deref())
            == nominal_declaration_identity(newtype_id, declaration_identity) =>
        {
            Ok((*inner).clone())
        }
        Value::Newtype { newtype_id: id, .. } => Err(fault(
            codes::GUARD_TYPE,
            format!("`.value()` needs a `{newtype_id}`, found a `{id}`"),
            span,
        )),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`.value()` needs a `{newtype_id}`, found a {}",
                other.kind()
            ),
            span,
        )),
    }
}

/// §8 record update — STAGE 2: merge the evaluated `fields` onto a clone of the
/// base record. An update may only OVERRIDE existing fields — a field name
/// absent from a NON-EMPTY base faults GUARD_NO_FIELD (an empty base, `{...{}}`,
/// adds, matching the interpreter's `updating > 0` guard). Fields apply in
/// source order (a later duplicate wins).
pub fn record_update_merge(
    base: Rc<BTreeMap<String, Value>>,
    fields: Vec<(String, Value)>,
    span: Span,
) -> Result<Value, RtError> {
    let mut map = (*base).clone();
    let updating = map.len();
    for (name, value) in fields {
        if updating > 0 && !map.contains_key(&name) {
            return Err(fault(
                codes::GUARD_NO_FIELD,
                format!("record update names unknown field `{name}`"),
                span,
            ));
        }
        map.insert(name, value);
    }
    Ok(Value::Record(Rc::new(map)))
}

/// §2/§12 short-circuit operators `&&` / `||` / `??` — the LHS-driven
/// decision, shared so the bool guard (`&&`/`||`), the `??` unwrap, and the
/// non-bool fault cannot drift. The LHS is already evaluated; `Some(v)` is
/// the short-circuit RESULT (the RHS must NOT be evaluated), `None` means
/// "evaluate the RHS and use its value". `&&`/`||` require a `bool` LHS (a
/// non-bool faults); the RHS is returned as-is (never re-checked), matching
/// the interpreter's `KBinaryRhs`. `??` returns the RHS on `null`/`None`,
/// unwraps one layer on `Some(v)`, and passes any other value through.
pub fn short_circuit_lhs(lhs: Value, op: BinaryOp, span: Span) -> Result<Option<Value>, RtError> {
    let logical_fault = |v: &Value| {
        fault(
            codes::GUARD_TYPE,
            format!("logical operand must be `bool`, found `{}`", v.kind()),
            span,
        )
    };
    match op {
        BinaryOp::And => match lhs {
            Value::Bool(false) => Ok(Some(Value::Bool(false))),
            Value::Bool(true) => Ok(None),
            other => Err(logical_fault(&other)),
        },
        BinaryOp::Or => match lhs {
            Value::Bool(true) => Ok(Some(Value::Bool(true))),
            Value::Bool(false) => Ok(None),
            other => Err(logical_fault(&other)),
        },
        BinaryOp::Coalesce => Ok(match lhs {
            Value::Null | Value::None => None,
            Value::Some(v) => Some((*v).clone()),
            other => Some(other),
        }),
        _ => unreachable!("short_circuit_lhs called with a non-short-circuit operator"),
    }
}

/// §1 index `object[index]` — the shared operation both engines call. Only
/// an array indexed by an `int` is indexable: an out-of-bounds index faults
/// FAULT_INDEX, a string is a GUARD_TYPE (use `s.scalars()`), and any other
/// object/index pair is a GUARD_TYPE. The object is evaluated before the
/// index (the caller passes them already-evaluated, in that order), matching
/// the interpreter's `KIndexApply`.
pub fn index_value(object: Value, index: Value, span: Span) -> Result<Value, RtError> {
    match (object, index) {
        (Value::Array(items), Value::Int(i)) => {
            let items = items.borrow();
            // `usize::try_from` (not an `as` cast) so a negative index OR one
            // that overflows `usize` (e.g. on a 32-bit target) faults rather
            // than wrapping past the bounds check.
            match usize::try_from(i) {
                Ok(u) if u < items.len() => Ok(items[u].clone()),
                _ => Err(fault(
                    codes::FAULT_INDEX,
                    format!(
                        "index {i} is out of bounds for an array of length {}",
                        items.len()
                    ),
                    span,
                )),
            }
        }
        (Value::Str(_), _) => Err(fault(
            codes::GUARD_TYPE,
            "strings are not indexable; use `s.scalars()` (§1)",
            span,
        )),
        (obj, idx) => Err(fault(
            codes::GUARD_TYPE,
            format!("cannot index `{}` with `{}`", obj.kind(), idx.kind()),
            span,
        )),
    }
}

/// The shared cell store backing `Value::Array`.
pub type ArrayStore = Rc<RefCell<Vec<Value>>>;

/// §9/§13a the array cell an index-ASSIGN targets: the backing store and the
/// validated slot index. Shared so the interpreter's `xs[i] = …` and the
/// emitted code locate the slot — and fault — identically: a non-`Array` base
/// (`GUARD_TYPE`), a non-`int` index (`GUARD_TYPE`), or an out-of-bounds index
/// (`FAULT_INDEX`). Distinct from [`index_value`] (the READ path), which has
/// its own §1/§13a messages and `usize::try_from` overflow guard.
pub fn index_slot(base: &Value, index: &Value, span: Span) -> Result<(ArrayStore, usize), RtError> {
    let Value::Array(items) = base else {
        return Err(fault(
            codes::GUARD_TYPE,
            format!(
                "cannot index-assign `{}`; only Array cells are index-assignable (§9)",
                base.kind()
            ),
            span,
        ));
    };
    let Value::Int(i) = index else {
        return Err(fault(
            codes::GUARD_TYPE,
            format!("array indices are `int`, found `{}`", index.kind()),
            span,
        ));
    };
    let len = items.borrow().len();
    if *i < 0 || *i as usize >= len {
        return Err(fault(
            codes::FAULT_INDEX,
            format!("index {i} out of bounds for length {len} (§13a)"),
            span,
        ));
    }
    Ok((items.clone(), *i as usize))
}

/// §4/§8 reads through a pure record-member chain to a leaf value — the shared
/// leaf behind a member-path read (`r.a.b`) at the point a compound/`??=`
/// assignment reads its current leaf. A non-record link faults `GUARD_TYPE`
/// ("has no assignable member"); a missing field faults `GUARD_NO_FIELD`
/// (exact field sets). With no fields the root is the leaf. Shared so the
/// interpreter's `r.a.b (op)= …` and the emitted code read — and fault —
/// identically.
pub fn walk_fields_value(root: &Value, fields: &[&str], span: Span) -> Result<Value, RtError> {
    let mut cur = root.clone();
    for field in fields {
        let Value::Record(map) = &cur else {
            return Err(fault(
                codes::GUARD_TYPE,
                format!("`{}` has no assignable member (§4)", cur.kind()),
                span,
            ));
        };
        let Some(inner) = map.get(*field) else {
            return Err(fault(
                codes::GUARD_NO_FIELD,
                format!("record has no field `{field}` (§8: exact field sets)"),
                span,
            ));
        };
        cur = inner.clone();
    }
    Ok(cur)
}

/// §4/§8 functionally replaces `current.f1.f2… = value` and returns the rebuilt
/// value (any compound op was already applied to the pre-RHS leaf read). With
/// no fields the value replaces `current` itself. Each record is cloned and the
/// leaf field reinserted (records are value types — §8); a non-record link
/// faults `GUARD_TYPE`, a missing field `GUARD_NO_FIELD`. Shared so the
/// interpreter's member-path write and the emitted code rebuild — and fault —
/// identically.
pub fn update_fields_value(
    current: &Value,
    fields: &[&str],
    value: Value,
    span: Span,
) -> Result<Value, RtError> {
    let Some((field, rest)) = fields.split_first() else {
        return Ok(value);
    };
    let Value::Record(map) = current else {
        return Err(fault(
            codes::GUARD_TYPE,
            format!("`{}` has no assignable member (§4)", current.kind()),
            span,
        ));
    };
    let Some(inner) = map.get(*field) else {
        return Err(fault(
            codes::GUARD_NO_FIELD,
            format!("record has no field `{field}` (§8: exact field sets)"),
            span,
        ));
    };
    let new_inner = update_fields_value(&inner.clone(), rest, value, span)?;
    let mut new_map = (**map).clone();
    new_map.insert((*field).to_string(), new_inner);
    Ok(Value::Record(Rc::new(new_map)))
}

/// §9 array spread `[...e]` — extend `acc` with the elements of a spread
/// operand. The operand must be an `Array` (a non-array faults GUARD_TYPE);
/// shared so the flatten and the fault cannot drift. Regular (non-spread)
/// elements are pushed by the caller in source order, so the accumulated
/// order (and any left-to-right fault) matches the interpreter's `KArray`.
pub fn array_spread_extend(acc: &mut Vec<Value>, value: Value, span: Span) -> Result<(), RtError> {
    match value {
        Value::Array(items) => {
            acc.extend(items.borrow().iter().cloned());
            Ok(())
        }
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("array spread needs an `Array`, found `{}`", other.kind()),
            span,
        )),
    }
}

/// §5 CALL / CONSTRUCTOR spread-extend — append a spread ARGUMENT's items to the
/// flattened argument accumulator, faulting with the call-spread message (distinct from
/// §9 array-literal `array_spread_extend`) when the spread value is not an Array. Shared
/// by the interpreter's `KCallArgs` spread arm and the emitter's `Array.of`/`Set.of`
/// spread lowering, so the §5 spread fault cannot drift between the two engines.
pub fn call_spread_extend(acc: &mut Vec<Value>, value: Value, span: Span) -> Result<(), RtError> {
    match value {
        Value::Array(items) => {
            acc.extend(items.borrow().iter().cloned());
            Ok(())
        }
        _ => Err(fault(
            codes::GUARD_TYPE,
            "a spread argument must be an Array (§5)",
            span,
        )),
    }
}

/// §10 range construction — the SINGLE shared builder both engines call
/// to turn finished endpoint/step values into a `Value::Range`, so the
/// int-endpoint and int/non-zero-step guards (and their faults) cannot
/// drift (CDR-006 §2). Endpoints are checked BEFORE the step, exactly as
/// the interpreter's `finish_range` does.
pub fn make_range(
    lo: Value,
    hi: Value,
    inclusive: bool,
    step: Option<Value>,
    span: Span,
) -> Result<Value, RtError> {
    let (Value::Int(lo), Value::Int(hi)) = (&lo, &hi) else {
        return Err(fault(
            codes::GUARD_TYPE,
            "range endpoints must be `int` in this build (§10)",
            span,
        ));
    };
    let step = match step {
        // §10: the effective step of an omitted `by` is always 1 — `5..1`
        // is empty; descending needs `by -1`.
        None => 1,
        Some(Value::Int(s)) => {
            if s == 0 {
                return Err(fault(
                    codes::FAULT_RANGE_STEP,
                    "range step must not be zero (§10)",
                    span,
                ));
            }
            s
        }
        Some(other) => {
            return Err(fault(
                codes::GUARD_TYPE,
                format!("range step must be `int`, found `{}`", other.kind()),
                span,
            ));
        }
    };
    Ok(Value::Range {
        lo: *lo,
        hi: *hi,
        inclusive,
        step,
    })
}

/// Decode SPEC §1 string escapes ({n,t,r,\,\",{,}}) into `out`.
/// The lexer validates the escape SET statically (TPZ0004), so for
/// a resolved program the `unsupported escape` fault is unreachable;
/// the emitter decodes at emit time on this shared leaf so the
/// decoded bytes cannot drift from the interpreter.
/// §1 escape resolution for string text runs.
pub fn decode_escapes(raw: &str, out: &mut String, span: Span) -> Result<(), RtError> {
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('{') => out.push('{'),
            Some('}') => out.push('}'),
            other => {
                return Err(fault(
                    codes::GUARD_TYPE,
                    format!(
                        "unsupported escape `\\{}`",
                        other.map(String::from).unwrap_or_default()
                    ),
                    span,
                ));
            }
        }
    }
    Ok(())
}
