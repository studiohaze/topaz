use super::*;

#[test]
fn json_stringify_canonical_encodes_and_rejects() {
    let j = |v: &Value| json_stringify(v, true);
    // scalars
    assert_eq!(j(&Value::Int(42)), Ok("42".to_string()));
    assert_eq!(j(&Value::Bool(true)), Ok("true".to_string()));
    assert_eq!(j(&Value::Unit), Ok("null".to_string()));
    assert_eq!(j(&Value::None), Ok("null".to_string()));
    assert_eq!(j(&Value::Some(Rc::new(Value::Int(7)))), Ok("7".to_string()));
    // string escaping
    assert_eq!(
        j(&Value::str("a\"b\\c\n")),
        Ok("\"a\\\"b\\\\c\\n\"".to_string())
    );
    // array preserves order
    assert_eq!(
        j(&Value::array(vec![Value::Int(1), Value::Int(2)])),
        Ok("[1,2]".to_string())
    );
    // record keys are canonical (sorted) — insert out of order, expect sorted
    let rec = Value::record([
        ("b".to_string(), Value::Int(2)),
        ("a".to_string(), Value::str("x")),
    ]);
    assert_eq!(j(&rec), Ok("{\"a\":\"x\",\"b\":2}".to_string()));
    // float / Set / function are rejected with a pathful message
    assert!(
        j(&Value::Float(1.5))
            .unwrap_err()
            .contains("float is not supported")
    );
    let mut set = OrderedSet::new();
    let _ = set.add_value(&Value::Int(1));
    assert!(
        json_stringify(&Value::Set(Rc::new(RefCell::new(set))), true)
            .unwrap_err()
            .contains("not JSON-encodable")
    );
}

#[test]
fn json_decode_unit_and_null_literal_are_distinct() {
    assert_eq!(
        render(&builtin_json_parse_as(Value::str("null"), &Schema::Unit, SP).unwrap()),
        "Ok(())"
    );
    assert_eq!(
        render(&builtin_json_parse_as(Value::str("null"), &Schema::Null, SP).unwrap()),
        "Ok(null)"
    );
}

#[test]
fn json_exact_int_handles_i128_exponent_boundaries_without_overflow() {
    fn number_int(text: &str) -> Option<i64> {
        match json_parse(text).expect("valid JSON number") {
            JsonValue::Number(number) => number.int,
            other => panic!("expected number, got {other:?}"),
        }
    }

    let i128_max = "170141183460469231731687303715884105727";
    assert_eq!(number_int(&format!("1e{i128_max}")), None);
    assert_eq!(number_int(&format!("0e{i128_max}")), Some(0));
    assert_eq!(
        number_int("0e9999999999999999999999999999999999999999"),
        None
    );
}

// --- Scalar checked-arith leaf (Part A, v5.4 native-emit substrate) ---
