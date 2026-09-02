use super::*;

#[test]
fn host_requirement_uses_the_resolved_builtin_identity() {
    let mut call = operation("expression/call", "");
    call.call_method = "readText".to_string();
    call.call_target = "user.reader::readText".to_string();
    assert!(!operation_requires_host(&call));

    call.call_target = "builtin::FS".to_string();
    assert!(operation_requires_host(&call));

    call.call_method.clear();
    call.call_target = "builtin::__lispexEvaluate".to_string();
    assert!(operation_requires_host(&call));
}

#[test]
fn target_facts_preserve_nominal_declaration_order() {
    let facts = r#"{
        "schema":"topaz.self-target-adapter-facts/v1",
        "nominals":[
            {"name":"Model","identity":"src.model::Model","kind":"record","members":[
                {"name":"source","arity":1},{"name":"status","arity":1}
            ]},
            {"name":"Msg","identity":"src.model::Msg","kind":"enum","members":[
                {"name":"Changed","arity":1},{"name":"Reset","arity":0}
            ]}
        ],
        "operationNominals":[]
    }"#;
    let program = Program {
        modules: Vec::new(),
        operations: Vec::new(),
    };
    let machine = Machine::new_with_facts(Arc::new(program), Some(facts)).expect("target facts");
    let member = operation("expression/member", "Reset");
    let Flow::Value(RuntimeValue::Data(Value::Enum {
        enum_id,
        variant,
        variant_index,
        payloads,
        ..
    })) = machine
        .eval_nominal_member(&member, "src.model::Msg")
        .expect("zero-payload variant")
    else {
        panic!("enum member did not construct a value");
    };
    assert_eq!(enum_id.as_ref(), "src.model::Msg");
    assert_eq!(variant.as_ref(), "Reset");
    assert_eq!(variant_index, 1);
    assert!(payloads.is_empty());

    let Flow::Value(RuntimeValue::Data(Value::NominalRecord {
        record_id, fields, ..
    })) = machine
        .construct_nominal(
            &operation("expression/call", ""),
            "Model",
            vec![
                RuntimeValue::Data(Value::str("text")),
                RuntimeValue::Data(Value::Int(2)),
            ],
        )
        .expect("nominal record")
    else {
        panic!("record constructor was not nominal");
    };
    assert_eq!(record_id.as_ref(), "src.model::Model");
    assert_eq!(fields[0].0.as_ref(), "source");
    assert_eq!(fields[1].0.as_ref(), "status");
}

#[test]
fn private_typed_json_null_decodes_as_none() {
    assert!(matches!(
        json_to_value(&JsonValue::Null).expect("JSON null"),
        Value::None
    ));
}

#[test]
fn embedded_program_parser_rejects_corrupt_magic() {
    let error = parse_embedded_program(b"NOT-C2BIN\x01{}", b"TPZC2BIN\x01", "Stage 2")
        .expect_err("corrupt image must fail closed");
    assert!(error.contains("magic"), "{error}");
}

#[test]
fn embedded_program_parser_rejects_an_impossible_operation_count_before_allocating() {
    let mut bytes = b"TPZC2BIN\x01".to_vec();
    compact_u32(&mut bytes, u32::MAX);
    let error = parse_embedded_program(&bytes, b"TPZC2BIN\x01", "Stage 2")
        .expect_err("impossible operation count must fail closed");
    assert!(
        error.contains("operation count exceeds remaining bytes"),
        "{error}"
    );
}

#[test]
fn compact_index_vector_rejects_an_impossible_length_before_allocating() {
    let mut bytes = Vec::new();
    compact_u32(&mut bytes, u32::MAX);
    let error = ProgramReader::new(&bytes)
        .indexes()
        .expect_err("impossible index vector length must fail closed");
    assert!(
        error.contains("index vector exceeds remaining bytes"),
        "{error}"
    );
}

#[test]
fn embedded_program_parser_rejects_duplicate_operation_ids() {
    let bytes = compact_program(&[
        ("same", "expression/integer", &[]),
        ("same", "expression/integer", &[]),
    ]);
    let error = parse_embedded_program(&bytes, b"TPZC2BIN\x01", "Stage 2")
        .expect_err("duplicate operation ids must fail closed");
    assert!(error.contains("duplicates operation id `same`"), "{error}");
}

#[test]
fn embedded_program_parser_rejects_an_out_of_range_operand() {
    let bytes = compact_program(&[("root", "expression/integer", &[1])]);
    let error = parse_embedded_program(&bytes, b"TPZC2BIN\x01", "Stage 2")
        .expect_err("out-of-range operand must fail closed");
    assert!(
        error.contains("operation operand is out of range"),
        "{error}"
    );
}

#[test]
fn embedded_program_parser_rejects_a_call_without_a_callee() {
    let bytes = compact_program(&[("call", "expression/call", &[])]);
    let error = parse_embedded_program(&bytes, b"TPZC2BIN\x01", "Stage 2")
        .expect_err("structurally incomplete call must fail closed");
    assert!(
        error.contains("expression/call) expects at least 1 operand"),
        "{error}"
    );
}

#[test]
fn shared_runtime_diagnostic_round_trips_through_self_product_context() {
    let original = topaz_value::fault(
        topaz_value::codes::FAULT_INDEX,
        "index failure\nwith context",
        Span::new(FileId(0), 7, 12),
    );
    let enriched = format!("{}\n  at sample", runtime_diagnostic(original.clone()));
    assert_eq!(decode_runtime_diagnostic(&enriched), Some(original));
}
