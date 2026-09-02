use super::*;

#[test]
fn int_arith_helpers_carry_the_exact_faults() {
    // Normal results.
    assert_eq!(int_add(2, 3, SP), Ok(5));
    assert_eq!(int_sub(2, 3, SP), Ok(-1));
    assert_eq!(int_mul(6, 7, SP), Ok(42));
    assert_eq!(int_div(7, 2, SP), Ok(3));
    assert_eq!(int_rem(7, 2, SP), Ok(1));
    assert_eq!(int_pow(2, 10, SP), Ok(1024));
    assert_eq!(int_neg(5, SP), Ok(-5));

    // Checked-overflow → TPZ4004 with the exact messages.
    let add_of = int_add(i64::MAX, 1, SP).unwrap_err();
    assert_eq!(add_of.code, codes::FAULT_OVERFLOW);
    assert_eq!(add_of.message, "integer addition overflows");
    assert_eq!(
        int_sub(i64::MIN, 1, SP).unwrap_err().code,
        codes::FAULT_OVERFLOW
    );
    assert_eq!(
        int_mul(i64::MAX, 2, SP).unwrap_err().code,
        codes::FAULT_OVERFLOW
    );
    assert_eq!(
        int_neg(i64::MIN, SP).unwrap_err().code,
        codes::FAULT_OVERFLOW
    );

    // Div/rem by zero → TPZ4002 with the exact messages.
    let dz = int_div(1, 0, SP).unwrap_err();
    assert_eq!(dz.code, codes::FAULT_DIV_ZERO);
    assert_eq!(dz.message, "integer division by zero");
    let rz = int_rem(1, 0, SP).unwrap_err();
    assert_eq!(rz.code, codes::FAULT_DIV_ZERO);
    assert_eq!(rz.message, "integer remainder by zero");

    // i64::MIN / -1 (and % -1) overflows → TPZ4004 (NOT a div-zero).
    assert_eq!(
        int_div(i64::MIN, -1, SP).unwrap_err().code,
        codes::FAULT_OVERFLOW
    );
    assert_eq!(
        int_rem(i64::MIN, -1, SP).unwrap_err().code,
        codes::FAULT_OVERFLOW
    );

    // Negative exponent → TPZ4005; overflowing exponent → TPZ4004.
    assert_eq!(
        int_pow(2, -1, SP).unwrap_err().code,
        codes::FAULT_NEG_EXPONENT
    );
    assert_eq!(
        int_pow(10, 100, SP).unwrap_err().code,
        codes::FAULT_OVERFLOW
    );
}

#[test]
fn math_leaves_pin_exact_float_outputs() {
    // ★ FLOAT DETERMINISM PIN. These exact rendered strings are the contract:
    // the interpreter and the emitted Rust BOTH call these same leaves, so a
    // change to any output here is a change to BOTH engines at once (run≡build),
    // and CI catches an accidental drift. `render` is the SAME float formatter
    // string interpolation `{x}` uses.
    let f = Value::Float;
    let r = |v: &Value| render(v);

    // sqrt — Ok on a non-negative; the principal root, full f64 precision.
    assert_eq!(
        r(&builtin_math_sqrt(f(2.0), SP).unwrap()),
        "Ok(1.4142135623730951)"
    );
    assert_eq!(r(&builtin_math_sqrt(f(9.0), SP).unwrap()), "Ok(3.0)");
    assert_eq!(r(&builtin_math_sqrt(f(0.0), SP).unwrap()), "Ok(0.0)");
    // sqrt(-0.0) = -0.0 (IEEE), an Ok — NOT a domain error.
    assert_eq!(r(&builtin_math_sqrt(f(-0.0), SP).unwrap()), "Ok(-0.0)");
    // sqrt of a NEGATIVE → Err (never NaN, never a fault).
    assert_eq!(
        r(&builtin_math_sqrt(f(-1.0), SP).unwrap()),
        "Err(Math.sqrt: domain error (argument -1 is negative))"
    );
    // sqrt of NaN → Err (a NaN argument is a domain error, not silently NaN).
    assert!(matches!(
        builtin_math_sqrt(f(f64::NAN), SP).unwrap(),
        Value::Err(_)
    ));

    // abs / floor / ceil — including -0.0 normalization (abs(-0.0)=0.0).
    assert_eq!(r(&builtin_math_abs(f(-2.5), SP).unwrap()), "2.5");
    assert_eq!(r(&builtin_math_abs(f(-0.0), SP).unwrap()), "0.0");
    // abs of NaN stays NaN (NaN != NaN, so check via the field + isNaN leaf).
    let abs_nan = builtin_math_abs(f(f64::NAN), SP).unwrap();
    assert!(matches!(abs_nan, Value::Float(x) if x.is_nan()));
    assert_eq!(r(&builtin_math_is_nan(abs_nan, SP).unwrap()), "true");
    assert_eq!(r(&builtin_math_floor(f(3.7), SP).unwrap()), "3.0");
    assert_eq!(r(&builtin_math_floor(f(-0.5), SP).unwrap()), "-1.0");
    assert_eq!(r(&builtin_math_ceil(f(3.2), SP).unwrap()), "4.0");
    assert_eq!(r(&builtin_math_ceil(f(-3.7), SP).unwrap()), "-3.0");

    // round — half AWAY from zero (the pinned half-way rule).
    assert_eq!(r(&builtin_math_round(f(2.5), SP).unwrap()), "3.0");
    assert_eq!(r(&builtin_math_round(f(-2.5), SP).unwrap()), "-3.0");
    assert_eq!(r(&builtin_math_round(f(0.5), SP).unwrap()), "1.0");
    assert_eq!(r(&builtin_math_round(f(2.4), SP).unwrap()), "2.0");

    // trig — radians, pinned through this shared leaf. These exact values are
    // intentionally conservative representatives of the platform libm contract.
    assert_eq!(r(&builtin_math_sin(f(0.0), SP).unwrap()), "0.0");
    assert_eq!(
        r(&builtin_math_sin(f(std::f64::consts::FRAC_PI_2), SP).unwrap()),
        "1.0"
    );
    assert_eq!(r(&builtin_math_cos(f(0.0), SP).unwrap()), "1.0");
    assert_eq!(
        r(&builtin_math_cos(f(std::f64::consts::PI), SP).unwrap()),
        "-1.0"
    );
    assert_eq!(r(&builtin_math_tan(f(0.0), SP).unwrap()), "0.0");
    assert_eq!(
        r(&builtin_math_tan(f(std::f64::consts::FRAC_PI_4), SP).unwrap()),
        "0.9999999999999999"
    );

    // min / max — via the shared `<`/`>` ordering; a NaN operand is asymmetric
    // but deterministic: a NaN makes the comparison false, so the result is the
    // SECOND operand — min(NaN,b)=b, min(a,NaN)=NaN, max(NaN,b)=b, max(a,NaN)=NaN.
    assert_eq!(r(&builtin_math_min(f(1.5), f(2.5), SP).unwrap()), "1.5");
    assert_eq!(r(&builtin_math_max(f(1.5), f(2.5), SP).unwrap()), "2.5");
    // NaN as the FIRST operand → the second operand (finite) is returned.
    assert_eq!(
        r(&builtin_math_min(f(f64::NAN), f(2.5), SP).unwrap()),
        "2.5"
    );
    assert_eq!(
        r(&builtin_math_max(f(f64::NAN), f(2.5), SP).unwrap()),
        "2.5"
    );
    // NaN as the SECOND operand → NaN is returned (NaN != NaN, so check via isNaN).
    assert!(matches!(
        builtin_math_min(f(1.5), f(f64::NAN), SP).unwrap(),
        Value::Float(x) if x.is_nan()
    ));
    assert!(matches!(
        builtin_math_max(f(1.5), f(f64::NAN), SP).unwrap(),
        Value::Float(x) if x.is_nan()
    ));

    // isNaN / isFinite — over NaN, ±inf, finite.
    assert_eq!(r(&builtin_math_is_nan(f(f64::NAN), SP).unwrap()), "true");
    assert_eq!(r(&builtin_math_is_nan(f(0.0), SP).unwrap()), "false");
    assert_eq!(r(&builtin_math_is_finite(f(1.0), SP).unwrap()), "true");
    assert_eq!(
        r(&builtin_math_is_finite(f(f64::INFINITY), SP).unwrap()),
        "false"
    );
    assert_eq!(
        r(&builtin_math_is_finite(f(f64::NAN), SP).unwrap()),
        "false"
    );

    // parseFloat — Ok on a numeric string (trimmed), Err otherwise.
    assert_eq!(
        r(&builtin_math_parse_float(Value::str("1.5"), SP).unwrap()),
        "Ok(1.5)"
    );
    assert_eq!(
        r(&builtin_math_parse_float(Value::str("  3.14  "), SP).unwrap()),
        "Ok(3.14)"
    );
    assert_eq!(
        r(&builtin_math_parse_float(Value::str("-0.0"), SP).unwrap()),
        "Ok(-0.0)"
    );
    // Exponent form parses to its exact finite value.
    assert_eq!(
        r(&builtin_math_parse_float(Value::str("1e10"), SP).unwrap()),
        "Ok(10000000000.0)"
    );
    // Leading/trailing whitespace is trimmed (pinned).
    assert_eq!(
        r(&builtin_math_parse_float(Value::str("  2.0  "), SP).unwrap()),
        "Ok(2.0)"
    );
    // Empty / non-numeric / trailing-junk → Err.
    assert!(matches!(
        builtin_math_parse_float(Value::str(""), SP).unwrap(),
        Value::Err(_)
    ));
    assert!(matches!(
        builtin_math_parse_float(Value::str("nope"), SP).unwrap(),
        Value::Err(_)
    ));
    assert!(matches!(
        builtin_math_parse_float(Value::str("1.5x"), SP).unwrap(),
        Value::Err(_)
    ));
    // ★ NON-FINITE spellings (any case) → Err: a text parse never mints inf/NaN.
    for spelling in [
        "inf", "Inf", "infinity", "Infinity", "-inf", "nan", "NaN", "NAN",
    ] {
        assert!(
            matches!(
                builtin_math_parse_float(Value::str(spelling), SP).unwrap(),
                Value::Err(_)
            ),
            "parseFloat({spelling:?}) must be Err (non-finite)"
        );
    }
}

#[test]
fn date_leaves_pin_iso_gregorian_math_and_keyability() {
    let ok = |v: Value| match v {
        Value::Ok(inner) => (*inner).clone(),
        o => panic!("expected Ok(...), got {}", render(&o)),
    };

    let leap =
        ok(builtin_date_from_ymd(Value::Int(2024), Value::Int(2), Value::Int(29), SP).unwrap());
    assert_eq!(render(&leap), "Date(2024-02-29)");
    assert_eq!(
        render(&builtin_date_to_iso(leap.clone(), SP).unwrap()),
        "2024-02-29"
    );
    let next = builtin_date_add_days(leap.clone(), Value::Int(1), SP).unwrap();
    assert_eq!(
        render(&builtin_date_to_iso(next.clone(), SP).unwrap()),
        "2024-03-01"
    );
    assert_eq!(
        render(&builtin_date_year(next.clone(), SP).unwrap()),
        "2024"
    );
    assert_eq!(render(&builtin_date_month(next.clone(), SP).unwrap()), "3");
    assert_eq!(render(&builtin_date_day(next, SP).unwrap()), "1");

    assert!(matches!(
        builtin_date_from_ymd(Value::Int(2023), Value::Int(2), Value::Int(29), SP).unwrap(),
        Value::Err(_)
    ));
    assert!(matches!(
        builtin_date_parse_iso(Value::str("2024-02-30"), SP).unwrap(),
        Value::Err(_)
    ));
    assert!(canonical_key(&leap).is_ok());
    assert_eq!(
        builtin_date_add_days(leap.clone(), Value::Int(i64::MAX), SP)
            .unwrap_err()
            .code,
        codes::FAULT_OVERFLOW
    );
    assert_eq!(
        render(
            &binary_value(
                BinaryOp::Lt,
                leap.clone(),
                ok(builtin_date_parse_iso(Value::str("2024-03-01"), SP).unwrap()),
                SP,
            )
            .unwrap()
        ),
        "true"
    );
}

#[test]
fn bigint_leaves_pin_radix_math_bounds_and_keyability() {
    let some = |v: Value| match v {
        Value::Some(inner) => (*inner).clone(),
        o => panic!("expected Some(...), got {}", render(&o)),
    };
    let ok = |v: Value| match v {
        Value::Ok(inner) => (*inner).clone(),
        o => panic!("expected Ok(...), got {}", render(&o)),
    };

    let huge =
        some(builtin_bigint_parse(Value::str("9223372036854775808"), Value::Int(10), SP).unwrap());
    assert_eq!(render(&huge), "BigInt(9223372036854775808)");
    assert_eq!(
        render(&builtin_bigint_to_int(huge.clone(), SP).unwrap()),
        "None"
    );
    assert_eq!(
        render(&builtin_bigint_to_string(huge.clone(), Value::Int(16), SP).unwrap()),
        "8000000000000000"
    );

    let min =
        some(builtin_bigint_parse(Value::str("-9223372036854775808"), Value::Int(10), SP).unwrap());
    assert_eq!(
        render(&builtin_bigint_to_int(min.clone(), SP).unwrap()),
        "Some(-9223372036854775808)"
    );
    assert_eq!(
        render(
            &binary_value(
                BinaryOp::Add,
                huge.clone(),
                Value::BigInt(Rc::new(BigIntData::from_i64(2))),
                SP,
            )
            .unwrap()
        ),
        "BigInt(9223372036854775810)"
    );
    assert_eq!(
        render(
            &binary_value(
                BinaryOp::Mul,
                Value::BigInt(Rc::new(BigIntData::from_i64(-7))),
                Value::BigInt(Rc::new(BigIntData::from_i64(6))),
                SP,
            )
            .unwrap()
        ),
        "BigInt(-42)"
    );

    let five = builtin_bigint_from_int(Value::Int(5), SP).unwrap();
    let two = builtin_bigint_from_int(Value::Int(2), SP).unwrap();
    let zero = builtin_bigint_from_int(Value::Int(0), SP).unwrap();
    assert_eq!(
        render(&ok(
            builtin_bigint_div(five.clone(), two.clone(), SP).unwrap()
        )),
        "BigInt(2)"
    );
    assert_eq!(
        render(&ok(builtin_bigint_mod(five.clone(), two, SP).unwrap())),
        "BigInt(1)"
    );
    assert!(matches!(
        builtin_bigint_div(five.clone(), zero, SP).unwrap(),
        Value::Err(_)
    ));
    assert!(matches!(
        builtin_bigint_parse(Value::str("2"), Value::Int(2), SP).unwrap(),
        Value::None
    ));
    assert_eq!(
        render(&binary_value(BinaryOp::Lt, min, huge.clone(), SP).unwrap()),
        "true"
    );
    assert!(canonical_key(&huge).is_ok());
}

#[test]
fn decimal_leaves_pin_parse_scale_math_and_keyability() {
    let some = |v: Value| match v {
        Value::Some(inner) => (*inner).clone(),
        o => panic!("expected Some(...), got {}", render(&o)),
    };
    let ok = |v: Value| match v {
        Value::Ok(inner) => (*inner).clone(),
        o => panic!("expected Ok(...), got {}", render(&o)),
    };

    let d = some(builtin_decimal_parse(Value::str("12.3400"), SP).unwrap());
    assert_eq!(render(&d), "Decimal(12.34)");
    assert_eq!(
        render(&builtin_decimal_to_string(d.clone(), SP).unwrap()),
        "12.34"
    );
    assert_eq!(render(&builtin_decimal_scale(d.clone(), SP).unwrap()), "2");
    assert_eq!(
        render(&builtin_decimal_to_int(d.clone(), SP).unwrap()),
        "None"
    );

    let whole = some(builtin_decimal_parse(Value::str("10.00"), SP).unwrap());
    assert_eq!(render(&whole), "Decimal(10)");
    assert_eq!(
        render(&builtin_decimal_to_int(whole.clone(), SP).unwrap()),
        "Some(10)"
    );
    assert_eq!(render(&builtin_decimal_scale(whole, SP).unwrap()), "0");

    let two = builtin_decimal_from_int(Value::Int(2), SP).unwrap();
    assert_eq!(
        render(&binary_value(BinaryOp::Add, d.clone(), two.clone(), SP).unwrap()),
        "Decimal(14.34)"
    );
    assert_eq!(
        render(&binary_value(BinaryOp::Sub, d.clone(), two.clone(), SP).unwrap()),
        "Decimal(10.34)"
    );
    assert_eq!(
        render(&binary_value(BinaryOp::Mul, d.clone(), two.clone(), SP).unwrap()),
        "Decimal(24.68)"
    );
    assert_eq!(
        render(&binary_value(BinaryOp::Lt, two, d.clone(), SP).unwrap()),
        "true"
    );

    assert!(canonical_key(&d).is_ok());
    assert!(matches!(
        builtin_decimal_parse(Value::str("1e2"), SP).unwrap(),
        Value::None
    ));
    assert!(matches!(
        builtin_decimal_parse(Value::str("12."), SP).unwrap(),
        Value::None
    ));
    assert!(matches!(
        builtin_decimal_parse(Value::str(" 12"), SP).unwrap(),
        Value::None
    ));

    let pos_half = some(builtin_decimal_parse(Value::str("2.5"), SP).unwrap());
    let neg_half = some(builtin_decimal_parse(Value::str("-2.5"), SP).unwrap());
    assert_eq!(
        render(
            &builtin_decimal_round(
                pos_half.clone(),
                Value::Int(0),
                rounding_mode_value(RoundingMode::HalfEven),
                SP
            )
            .unwrap()
        ),
        "Decimal(2)"
    );
    assert_eq!(
        render(
            &builtin_decimal_round(
                pos_half.clone(),
                Value::Int(0),
                rounding_mode_value(RoundingMode::HalfUp),
                SP
            )
            .unwrap()
        ),
        "Decimal(3)"
    );
    assert_eq!(
        render(
            &builtin_decimal_round(
                neg_half,
                Value::Int(0),
                rounding_mode_value(RoundingMode::Down),
                SP
            )
            .unwrap()
        ),
        "Decimal(-3)"
    );
    let half_even = rounding_mode_value(RoundingMode::HalfEven);
    assert_eq!(
        values_equal(&half_even, &rounding_mode_value(RoundingMode::HalfEven)),
        Ok(true)
    );
    assert_eq!(
        values_compare(&half_even, &rounding_mode_value(RoundingMode::HalfUp)).err(),
        Some(CmpError::NotComparable("RoundingMode"))
    );
    assert_eq!(
        canonical_key(&half_even).err(),
        Some(CmpError::NotComparable("RoundingMode"))
    );
    let Value::Err(json_err) = builtin_json_stringify(half_even) else {
        panic!("expected RoundingMode JSON rejection");
    };
    assert!(matches!(&*json_err, Value::Str(s) if s.contains("RoundingMode")));

    let one = builtin_decimal_from_int(Value::Int(1), SP).unwrap();
    let eight = builtin_decimal_from_int(Value::Int(8), SP).unwrap();
    assert_eq!(
        render(&ok(builtin_decimal_div(
            one.clone(),
            eight.clone(),
            Value::Int(2),
            rounding_mode_value(RoundingMode::HalfEven),
            SP
        )
        .unwrap())),
        "Decimal(0.12)"
    );
    assert_eq!(
        render(&ok(builtin_decimal_div(
            one.clone(),
            eight,
            Value::Int(2),
            rounding_mode_value(RoundingMode::HalfUp),
            SP
        )
        .unwrap())),
        "Decimal(0.13)"
    );
    assert!(matches!(
        builtin_decimal_div(
            one,
            builtin_decimal_from_int(Value::Int(0), SP).unwrap(),
            Value::Int(2),
            rounding_mode_value(RoundingMode::HalfEven),
            SP
        )
        .unwrap(),
        Value::Err(_)
    ));
}

#[test]
fn scalar_helpers_match_binary_value_byte_for_byte() {
    // The behavior-preserving refactor proof: `binary_value` and the bare
    // scalar leaf agree on result AND on the fault (code, message, span).
    use BinaryOp::*;
    let cases: &[(BinaryOp, i64, i64)] = &[
        (Add, 2, 3),
        (Add, i64::MAX, 1),
        (Sub, i64::MIN, 1),
        (Mul, i64::MAX, 2),
        (Div, 7, 2),
        (Div, 1, 0),
        (Div, i64::MIN, -1),
        (Rem, 7, 2),
        (Rem, 1, 0),
        (Rem, i64::MIN, -1),
        (Pow, 2, 10),
        (Pow, 2, -1),
        (Pow, 10, 100),
    ];
    for &(op, a, b) in cases {
        let via_binary = binary_value(op, Value::Int(a), Value::Int(b), SP);
        let via_leaf = match op {
            Add => int_add(a, b, SP),
            Sub => int_sub(a, b, SP),
            Mul => int_mul(a, b, SP),
            Div => int_div(a, b, SP),
            Rem => int_rem(a, b, SP),
            Pow => int_pow(a, b, SP),
            _ => unreachable!(),
        }
        .map(Value::Int);
        match (via_binary, via_leaf) {
            (Ok(Value::Int(x)), Ok(Value::Int(y))) => assert_eq!(x, y, "{op:?}({a},{b})"),
            (Err(eb), Err(el)) => {
                assert_eq!(eb.code, el.code, "{op:?}({a},{b}) code");
                assert_eq!(eb.message, el.message, "{op:?}({a},{b}) message");
                assert_eq!(eb.span, el.span, "{op:?}({a},{b}) span");
            }
            (vb, vl) => panic!("{op:?}({a},{b}) diverged: {vb:?} vs {vl:?}"),
        }
    }
}

#[test]
fn cmp_and_float_helpers_match_their_semantics() {
    use BinaryOp::*;
    assert!(int_cmp(Lt, 1, 2));
    assert!(!int_cmp(Lt, 2, 1));
    assert!(int_cmp(Ge, 2, 2));
    assert_eq!(float_arith(Add, 1.5, 2.0), 3.5);
    assert!(float_arith(Div, 1.0, 0.0).is_infinite());
    let noncanonical_nan = f64::from_bits(0xfff8_0000_0000_0042);
    for (op, a, b) in [
        (Add, noncanonical_nan, 1.0),
        (Sub, noncanonical_nan, 1.0),
        (Mul, noncanonical_nan, 1.0),
        (Div, 0.0, 0.0),
        (Pow, -1.0, 0.5),
    ] {
        assert_eq!(
            float_arith(op, a, b).to_bits(),
            CANONICAL_ARITHMETIC_NAN_BITS,
            "{op:?} must canonicalize an arithmetic NaN"
        );
    }
    assert!(float_cmp(Lt, 1.0, 2.0));
    // NaN is unordered: every comparison is false.
    assert!(!float_cmp(Lt, f64::NAN, 1.0));
    assert!(!float_cmp(Ge, f64::NAN, 1.0));
}
