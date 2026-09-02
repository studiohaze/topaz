use super::*;

#[test]
fn canonical_abi_round_trips_supported_values_and_wraps_outcomes() {
    let value = Value::record([
        (
            "items".to_string(),
            Value::array(vec![
                Value::Int(i64::MAX),
                Value::Some(Rc::new(Value::str("Ada"))),
                Value::Bytes(Rc::from([0_u8, 255, 16].as_slice())),
                Value::NominalRecord {
                    record_id: Rc::from("User"),
                    declaration_identity: None,
                    method_identity: None,
                    fields: Rc::from(
                        vec![
                            (Rc::from("name"), Value::str("Ada")),
                            (Rc::from("age"), Value::Int(36)),
                        ]
                        .into_boxed_slice(),
                    ),
                },
                Value::Enum {
                    enum_id: Rc::from("MaybeInt"),
                    declaration_identity: None,
                    method_identity: None,
                    variant: Rc::from("Some"),
                    variant_index: 1,
                    payloads: Rc::from(vec![Value::Int(7)].into_boxed_slice()),
                },
                Value::Newtype {
                    newtype_id: Rc::from("UserId"),
                    declaration_identity: None,
                    method_identity: None,
                    inner: Rc::new(Value::Int(42)),
                },
            ]),
        ),
        ("ok".to_string(), Value::Ok(Rc::new(Value::Bool(true)))),
    ]);
    let encoded = canonical_abi_encode(&value).expect("ABI encode");
    assert_eq!(
        encoded,
        "{\"$\":\"record\",\"fields\":{\"items\":{\"$\":\"array\",\"items\":[{\"$\":\"int\",\"value\":\"9223372036854775807\"},{\"$\":\"some\",\"value\":{\"$\":\"string\",\"value\":\"Ada\"}},{\"$\":\"bytes\",\"hex\":\"00ff10\"},{\"$\":\"nominal-record\",\"id\":\"User\",\"fields\":[{\"name\":\"name\",\"value\":{\"$\":\"string\",\"value\":\"Ada\"}},{\"name\":\"age\",\"value\":{\"$\":\"int\",\"value\":\"36\"}}]},{\"$\":\"enum\",\"id\":\"MaybeInt\",\"variant\":\"Some\",\"index\":\"1\",\"payloads\":[{\"$\":\"int\",\"value\":\"7\"}]},{\"$\":\"newtype\",\"id\":\"UserId\",\"value\":{\"$\":\"int\",\"value\":\"42\"}}]},\"ok\":{\"$\":\"ok\",\"value\":{\"$\":\"bool\",\"value\":true}}}}"
    );
    assert_eq!(
        render(&canonical_abi_decode(&encoded).expect("ABI decode")),
        render(&value)
    );

    let args = canonical_abi_decode_args(&format!("[{encoded}]")).expect("ABI args");
    assert_eq!(args.len(), 1);
    assert_eq!(render(&args[0]), render(&value));

    assert_eq!(
        canonical_abi_completed(&Value::Int(7)),
        "{\"status\":\"ok\",\"value\":{\"$\":\"int\",\"value\":\"7\"}}"
    );
    assert_eq!(
        canonical_abi_error("bad\narg"),
        "{\"status\":\"error\",\"message\":\"bad\\narg\"}"
    );
}

#[test]
fn canonical_abi_rejects_noncanonical_or_unsupported_values() {
    assert!(
        canonical_abi_encode(&Value::Float(1.0))
            .unwrap_err()
            .contains("not ABI-encodable")
    );
    assert!(
        canonical_abi_decode("{\"$\":\"int\",\"value\":\"01\"}")
            .unwrap_err()
            .contains("canonical decimal")
    );
    assert!(
        canonical_abi_decode("{\"$\":\"bytes\",\"hex\":\"00FF\"}")
            .unwrap_err()
            .contains("non-lowercase-hex")
    );
    assert!(
            canonical_abi_decode(
                "{\"$\":\"nominal-record\",\"id\":\"User\",\"fields\":[{\"name\":\"x\",\"value\":{\"$\":\"int\",\"value\":\"1\"}},{\"name\":\"x\",\"value\":{\"$\":\"int\",\"value\":\"2\"}}]}"
            )
            .unwrap_err()
            .contains("duplicates")
        );
    assert!(
        canonical_abi_decode(
            "{\"$\":\"enum\",\"id\":\"E\",\"variant\":\"V\",\"index\":\"01\",\"payloads\":[]}"
        )
        .unwrap_err()
        .contains("canonical decimal")
    );
    assert!(
        canonical_abi_decode_args("{\"$\":\"unit\"}")
            .unwrap_err()
            .contains("expected an array")
    );
}
