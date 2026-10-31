use super::*;
use crate::value::compare::sort_values_stable;

fn nan_items() -> Vec<Value> {
    (0..64)
        .map(|index| Value::Float(if index % 11 == 3 { f64::NAN } else { (index * 37 % 101) as f64 }))
        .collect()
}

#[test]
fn nan_sorted_and_sort_return_guard_without_mutation() {
    for method in ["sort", "sorted"] {
        let receiver = Value::array(nan_items());
        let Value::Array(items) = &receiver else { unreachable!() };
        let before = render(&receiver);
        let error = call_method(receiver.clone(), method, vec![], SP, SP).unwrap_err();
        assert_eq!(error.code, codes::GUARD_COMPARE);
        assert!(error.message.contains("NaN"));
        assert_eq!(render(&receiver), before);
        assert_eq!(items.borrow().len(), 64);
    }
}

#[test]
fn callback_keys_reject_nan_and_misalignment() {
    let items = vec![Value::str("a"), Value::str("b")];
    assert_eq!(sorted_by_keys(&items, &[Value::Float(1.0), Value::Float(f64::NAN)], SP).unwrap_err().code,
               codes::GUARD_COMPARE);
    assert_eq!(sorted_by_keys(&items, &[], SP).unwrap_err().code, codes::GUARD_COMPARE);
}

#[test]
fn stable_permutation_and_signed_zero_are_preserved() {
    let items = vec![Value::str("a"), Value::str("b"), Value::str("c"), Value::str("d")];
    let sorted = sorted_by_keys(&items, &[Value::Int(2), Value::Int(1), Value::Int(2), Value::Int(1)], SP).unwrap();
    assert_eq!(render(&Value::array(sorted)), "[b, d, a, c]");
    let mut values = vec![Value::Int(3), Value::Int(1), Value::Int(2)];
    sort_values_stable(&mut values, SP, values_compare).unwrap();
    assert_eq!(render(&Value::array(values)), "[1, 2, 3]");
    assert_eq!(values_compare(&Value::Float(-0.0), &Value::Float(0.0)).unwrap(), std::cmp::Ordering::Equal);
}

#[test]
fn ieee_relations_and_equality_keep_nan_semantics() {
    let nan = Value::Float(f64::NAN);
    assert!(!values_equal(&nan, &nan).unwrap());
    assert!(!float_cmp(BinaryOp::Lt, f64::NAN, 0.0));
    assert!(!float_cmp(BinaryOp::Ge, f64::NAN, 0.0));
}
