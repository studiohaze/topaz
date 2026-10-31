use super::*;

#[test]
fn target_runtime_dispatches_receiver_manual_and_derived_protocol_methods() {
    let facts = r#"{
        "schema":"topaz.self-target-adapter-facts/v1",
        "nominals":[
            {"name":"Point","identity":"test::Point","kind":"record","members":[
                {"name":"value","arity":1}
            ]}
        ],
        "operationNominals":[{"operationId":"point-value","identity":"test::Point","kind":"record"}]
    }"#;

    let mut receiver_impl = operation("implementation", "Point");
    receiver_impl.operands = vec![1];
    let mut receiver_method = operation("function", "shifted");
    receiver_method.binding_name = "shifted".to_string();
    receiver_method.operands = vec![2, 3, 4];
    let mut receiver_parameter = operation("binding/parameter", "self");
    receiver_parameter.binding_name = "self".to_string();
    let mut receiver_delta = operation("binding/parameter", "delta");
    receiver_delta.binding_name = "delta".to_string();
    let mut receiver_body = operation("expression/block", "");
    receiver_body.operands = vec![5];
    let receiver_result = operation("expression/integer", "11");

    let mut protocol_impl = operation("implementation", "Shift<Point>");
    protocol_impl.operands = vec![7];
    let mut protocol_method = operation("function", "shift");
    protocol_method.binding_name = "shift".to_string();
    protocol_method.operands = vec![8, 9, 10];
    let mut protocol_value = operation("binding/parameter", "value");
    protocol_value.binding_name = "value".to_string();
    let mut protocol_delta = operation("binding/parameter", "delta");
    protocol_delta.binding_name = "delta".to_string();
    let mut protocol_body = operation("expression/block", "");
    protocol_body.operands = vec![11];
    let protocol_result = operation("expression/integer", "22");

    let mut receiver_call = operation("expression/call", "");
    receiver_call.operands = vec![13, 15];
    receiver_call.call_target = "test::Point".to_string();
    receiver_call.call_method = "shifted".to_string();
    let mut receiver_member = operation("expression/member", "shifted");
    receiver_member.operands = vec![14];
    let point_identifier = operation("expression/identifier", "point");
    let receiver_argument = operation("expression/integer", "2");

    let mut protocol_call = operation("expression/call", "");
    protocol_call.operands = vec![17, 19, 20];
    protocol_call.call_target = "builtin::Shift".to_string();
    protocol_call.call_method = "shift".to_string();
    let mut protocol_member = operation("expression/member", "shift");
    protocol_member.operands = vec![18];
    let protocol_identifier = operation("expression/identifier", "Shift");
    let protocol_point = operation("expression/identifier", "point");
    let protocol_argument = operation("expression/integer", "2");

    let mut derived_call = operation("expression/call", "");
    derived_call.operands = vec![22, 24, 25];
    derived_call.call_target = "builtin::Eq".to_string();
    derived_call.call_method = "equals".to_string();
    let mut derived_member = operation("expression/member", "equals");
    derived_member.operands = vec![23];
    let derived_identifier = operation("expression/identifier", "Eq");
    let derived_left = operation("expression/identifier", "point");
    let derived_right = operation("expression/identifier", "point");

    let mut point_literal = operation("expression/record-literal", "");
    point_literal.id = "point-value".into();
    point_literal.operands = vec![27];
    point_literal.operand_labels = vec!["field-initializer[0]=value".into()];
    let point_value = operation("expression/integer", "7");
    let program = Program::new(
        Vec::new(),
        vec![
            receiver_impl,
            receiver_method,
            receiver_parameter,
            receiver_delta,
            receiver_body,
            receiver_result,
            protocol_impl,
            protocol_method,
            protocol_value,
            protocol_delta,
            protocol_body,
            protocol_result,
            receiver_call,
            receiver_member,
            point_identifier,
            receiver_argument,
            protocol_call,
            protocol_member,
            protocol_identifier,
            protocol_point,
            protocol_argument,
            derived_call,
            derived_member,
            derived_identifier,
            derived_left,
            derived_right,
            point_literal,
            point_value,
        ],
    );
    let mut machine =
        Machine::new_with_facts(Arc::new(program), Some(facts)).expect("nominal target facts");
    machine.register_functions().expect("method registries");
    let environment = EnvironmentFrame::root();
    environment.define(
        "point".to_string(),
        run_local(machine.eval_value(26, environment.clone())).expect("nominal record literal"),
    );
    let receiver_declaration = data(
        run_local(machine.eval_value(0, environment.clone()))
            .expect("receiver implementation declaration"),
    )
    .expect("receiver implementation value");
    let protocol_declaration = data(
        run_local(machine.eval_value(6, environment.clone()))
            .expect("protocol implementation declaration"),
    )
    .expect("protocol implementation value");
    let receiver =
        data(run_local(machine.eval_value(12, environment.clone())).expect("receiver method call"))
            .expect("receiver result");
    let protocol =
        data(run_local(machine.eval_value(16, environment.clone())).expect("manual protocol call"))
            .expect("manual protocol result");
    let derived =
        data(run_local(machine.eval_value(21, environment)).expect("derived protocol call"))
            .expect("derived protocol result");
    assert_eq!(
        (
            topaz_value::value::render(&receiver),
            topaz_value::value::render(&protocol),
            topaz_value::value::render(&derived),
            topaz_value::value::render(&receiver_declaration),
            topaz_value::value::render(&protocol_declaration),
            machine.functions.contains_key("test::shifted"),
            machine.functions.contains_key("test::shift"),
        ),
        (
            "11".to_string(),
            "22".to_string(),
            "true".to_string(),
            "()".to_string(),
            "()".to_string(),
            false,
            false,
        ),
    );
}

#[test]
fn byte_buffer_static_calls_use_the_shared_value_contract() {
    let mut allocate = operation("expression/call", "ByteBuffer.allocate");
    allocate.call_method = "allocate".to_string();
    let mut from_bytes = operation("expression/call", "ByteBuffer.fromBytes");
    from_bytes.call_method = "fromBytes".to_string();
    let program = Program::new(Vec::new(), Vec::new());
    let mut machine = Machine::new(Arc::new(program));

    let Flow::Value(RuntimeValue::Data(buffer)) = run_local(machine.call_static(
        &allocate,
        "ByteBuffer",
        vec![Value::Int(3), Value::Int(17)],
    ))
    .expect("ByteBuffer.allocate") else {
        panic!("ByteBuffer.allocate did not return data");
    };
    assert_eq!(
        topaz_value::value::render(
            &topaz_value::value::builtin_byte_buffer_to_bytes(buffer, span(&allocate))
                .expect("ByteBuffer snapshot")
        ),
        "Bytes(111111)"
    );

    let Flow::Value(RuntimeValue::Data(copied)) = run_local(machine.call_static(
        &from_bytes,
        "ByteBuffer",
        vec![Value::Bytes(Rc::from([1_u8, 2, 3]))],
    ))
    .expect("ByteBuffer.fromBytes") else {
        panic!("ByteBuffer.fromBytes did not return data");
    };
    assert_eq!(
        topaz_value::value::render(
            &topaz_value::value::builtin_byte_buffer_to_bytes(copied, span(&from_bytes))
                .expect("copied ByteBuffer snapshot")
        ),
        "Bytes(010203)"
    );
}

#[test]
fn byte_buffer_static_calls_reject_invalid_arguments_without_fallback() {
    let mut allocate = operation("expression/call", "ByteBuffer.allocate");
    allocate.call_method = "allocate".to_string();
    let program = Program::new(Vec::new(), Vec::new());
    let mut machine = Machine::new(Arc::new(program));

    let arity = run_local(machine.call_static(&allocate, "ByteBuffer", Vec::new()))
        .err()
        .expect("missing length must fail");
    assert_eq!(
        arity,
        "topaz.runtime-diagnostic/v2\tTPZ5004\t0\t0\t1\tmissing argument for parameter `length` (§5)"
    );
    let byte = run_local(machine.call_static(
        &allocate,
        "ByteBuffer",
        vec![Value::Int(1), Value::Int(256)],
    ))
    .err()
    .expect("invalid byte must fail");
    assert!(byte.contains("byte value must be in 0..255"), "{byte}");
}

#[test]
fn runtime_faults_name_the_source_file_of_the_faulting_module() {
    // Synthetic module IDs exercise transport of a nonzero FileId. The CLI
    // tests compile imports and check discovery order against rendered files.
    let mut allocate = operation("expression/call", "ByteBuffer.allocate");
    allocate.module = "main".to_string();
    allocate.lo = 40;
    allocate.hi = 62;
    allocate.call_method = "allocate".to_string();
    let program = Arc::new(Program::new(
        vec![
            Module {
                generated_std: true,
                identity: "std.io".to_string(),
                source_ordinal: 0,
                entry: false,
                operations: Vec::new(),
            },
            Module {
                generated_std: false,
                identity: "main".to_string(),
                source_ordinal: 1,
                entry: true,
                operations: vec![0],
            },
        ],
        vec![allocate],
    ));
    let allocate = program.operations[0].clone();
    let mut machine = Machine::new(program);

    let error = run_local(machine.call_static(&allocate, "ByteBuffer", Vec::new()))
        .err()
        .expect("missing length must fail");
    let diagnostic = decode_runtime_diagnostic(&error).expect("runtime diagnostic");
    assert_eq!(
        (diagnostic.code, diagnostic.span),
        (
            topaz_value::codes::GUARD_ARITY,
            Span::new(FileId(1), 40, 62)
        )
    );
}
