use super::*;

#[test]
fn index_assignment_rejects_indices_that_cannot_fit_the_target() {
    let array = Value::array(vec![Value::Int(7)]);
    for index in [-1, 1, 1_i64 << 32, i64::MAX] {
        let error = index_slot(&array, &Value::Int(index), SP).unwrap_err();
        assert_eq!(error.code, codes::FAULT_INDEX);
    }
    assert_eq!(index_slot(&array, &Value::Int(0), SP).unwrap().1, 0);
    assert_eq!(render(&array), "[7]");
}

#[test]
fn optional_index_accessors_do_not_truncate_large_integers() {
    let receivers = [
        (Value::array(vec![Value::Int(7)]), "get"),
        (Value::Bytes(Rc::from([7_u8].as_slice())), "get"),
        (Value::Json(Rc::new(json_parse("[7]").unwrap())), "at"),
    ];
    for (receiver, method) in receivers {
        for index in [-1, 1, 1_i64 << 32, i64::MAX] {
            assert!(matches!(
                call_method(receiver.clone(), method, vec![Value::Int(index)], SP, SP).unwrap(),
                Value::None
            ));
        }
        assert!(matches!(
            call_method(receiver, method, vec![Value::Int(0)], SP, SP).unwrap(),
            Value::Some(_)
        ));
    }
}

#[test]
fn array_element_guards_observe_appends_and_alias_writes() {
    let value = Value::array(vec![Value::Int(1)]);
    let alias = value.clone();
    assert!(array_elements_match(&value, StableArrayElement::Int));
    let Value::Array(items) = &alias else {
        panic!("array");
    };
    items.push(Value::Int(2));
    assert!(array_elements_match(&value, StableArrayElement::Int));
    items.push(Value::str("bad"));
    assert!(!array_elements_match(&value, StableArrayElement::Int));
    items.borrow_mut()[2] = Value::Int(3);
    assert!(array_elements_match(&value, StableArrayElement::Int));
    items.borrow_mut()[0] = Value::Bool(false);
    assert!(!array_elements_match(&value, StableArrayElement::Int));
    *items.borrow_mut() = vec![Value::Bool(true)];
    assert!(array_elements_match(&value, StableArrayElement::Bool));
    assert!(!array_elements_match(&value, StableArrayElement::Int));
    items.borrow_mut().clear();
    assert!(array_elements_match(&value, StableArrayElement::Int));
    items.push(Value::Bool(true));
    assert!(!array_elements_match(&value, StableArrayElement::Int));
    assert!(array_elements_match(&value, StableArrayElement::Bool));
    assert!(!array_elements_match(
        &Value::Int(1),
        StableArrayElement::Int
    ));
}

#[test]
fn array_element_guards_keep_nominal_declarations_distinct() {
    fn record(id: &str) -> Value {
        Value::NominalRecord {
            record_id: Rc::from("Item"),
            declaration_identity: Some(Rc::from(id)),
            method_identity: None,
            fields: Rc::from([]),
        }
    }
    let value = Value::array(vec![record("first::Item")]);
    assert!(array_elements_match(
        &value,
        StableArrayElement::NominalRecord("first::Item")
    ));
    assert!(!array_elements_match(
        &value,
        StableArrayElement::NominalRecord("second::Item")
    ));
    let Value::Array(items) = &value else {
        panic!("array");
    };
    items.push(record("second::Item"));
    assert!(!array_elements_match(
        &value,
        StableArrayElement::NominalRecord("first::Item")
    ));
    items.borrow_mut().remove(0);
    assert!(array_elements_match(
        &value,
        StableArrayElement::NominalRecord("second::Item")
    ));
}
