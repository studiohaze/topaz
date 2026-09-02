use super::*;

#[test]
fn lexical_slot_precedes_colliding_module_function() {
    let mut identifier = operation("expression/identifier", "children");
    identifier.reference_identity = "checker::children".to_string();
    let program = Program {
        modules: Vec::new(),
        operations: vec![identifier.clone()],
    };
    let mut machine = Machine::new(Arc::new(program));
    Rc::get_mut(&mut machine.functions)
        .expect("new machine owns its function table")
        .insert("checker::children".to_string(), 0);
    let environment = EnvironmentFrame::root();
    environment.define("children".to_string(), RuntimeValue::Data(Value::Int(42)));
    let Flow::Value(RuntimeValue::Data(Value::Int(value))) = machine
        .eval_identifier(&identifier, environment)
        .expect("lexical identifier")
    else {
        panic!("lexical value did not win");
    };
    assert_eq!(value, 42);
}

#[test]
fn optional_member_uses_the_shared_optional_value_semantics() {
    let mut optional = operation("expression/optional-member", "name");
    optional.operands.push(1);
    let identifier = operation("expression/identifier", "receiver");
    let program = Program {
        modules: Vec::new(),
        operations: vec![optional, identifier],
    };
    let mut machine = Machine::new(Arc::new(program));
    let environment = EnvironmentFrame::root();
    environment.define(
        "receiver".to_string(),
        RuntimeValue::Data(Value::Some(Rc::new(Value::record(vec![(
            "name".to_string(),
            Value::str("Topaz"),
        )])))),
    );
    let Flow::Value(RuntimeValue::Data(Value::Some(value))) =
        machine.eval(0, environment).expect("optional member")
    else {
        panic!("optional member did not preserve Some");
    };
    assert!(matches!(value.as_ref(), Value::Str(value) if value.as_ref() == "Topaz"));
}

#[test]
fn map_new_uses_the_shared_ordered_map_constructor() {
    let mut call = operation("expression/call", "Map.new");
    call.call_method = "new".to_string();
    let machine = Machine::new(Arc::new(Program {
        modules: Vec::new(),
        operations: Vec::new(),
    }));
    let Flow::Value(RuntimeValue::Data(Value::Map(map))) = machine
        .call_static(&call, "Map", Vec::new())
        .expect("Map.new")
    else {
        panic!("Map.new did not produce a map");
    };
    assert!(map.borrow().is_empty());
}

#[test]
fn comprehension_rejects_a_body_for_another_collection_kind() {
    let mut comprehension = operation("expression/comprehension", "array");
    comprehension.operands = vec![1, 2];
    comprehension.operand_labels = vec!["bodyKey:0".to_string(), "bodyValue:0".to_string()];
    let mut machine = Machine::new(Arc::new(Program {
        modules: Vec::new(),
        operations: vec![
            comprehension,
            operation("expression/integer", "1"),
            operation("expression/integer", "2"),
        ],
    }));
    let error = match machine.eval(0, EnvironmentFrame::root()) {
        Err(error) => error,
        Ok(_) => panic!("array comprehension accepted a map-entry body"),
    };
    assert_eq!(
        error,
        concat!(
            "test:expression/comprehension:array comprehension body does not match its collection kind\n",
            "  at test:0-1 expression/comprehension (test:expression/comprehension:array)"
        )
    );
}

#[test]
fn typed_option_pattern_recovers_structural_json_payload() {
    let mut pattern = operation("pattern/constructor", "Some");
    pattern.operands.push(1);
    let mut binding = operation("pattern/binding", "payload");
    binding.binding_name = "payload".to_string();
    binding.declaration_identity = "source:0:0:1".to_string();
    let program = Program {
        modules: Vec::new(),
        operations: vec![pattern, binding],
    };
    let mut machine = Machine::new(Arc::new(program));
    let environment = EnvironmentFrame::root();
    assert!(
        run_local(
            machine.match_pattern(0, RuntimeValue::Data(Value::Int(7)), environment.clone(),)
        )
        .expect("typed Option match")
    );
    let RuntimeValue::Data(Value::Int(value)) = environment
        .slot("payload")
        .expect("payload slot")
        .borrow()
        .clone()
    else {
        panic!("typed Option payload was not bound");
    };
    assert_eq!(value, 7);
}

#[test]
fn generic_record_literal_uses_its_nominal_type_base() {
    let facts = r#"{
        "schema":"topaz.self-target-adapter-facts/v1",
        "nominals":[
            {"name":"Element","identity":"std.dom::Element","kind":"record","members":[
                {"name":"tag","arity":1},{"name":"children","arity":1}
            ]}
        ],
        "operationNominals":[]
    }"#;
    let mut record = operation("expression/record-update", "");
    record.operands = vec![1, 2, 3];
    record.operand_labels = vec![
        "base:0".to_string(),
        "field-initializer[0]=tag/fields:0/value:0".to_string(),
        "field-initializer[1]=children/fields:1/value:0".to_string(),
    ];
    let base = operation("expression/identifier", "Element");
    let tag = operation("expression/identifier", "tag");
    let children = operation("expression/identifier", "children");
    let mut machine = Machine::new_with_facts(
        Arc::new(Program {
            modules: Vec::new(),
            operations: vec![record, base, tag, children],
        }),
        Some(facts),
    )
    .expect("target facts");
    let environment = EnvironmentFrame::root();
    environment.define("tag".to_string(), RuntimeValue::Data(Value::str("section")));
    environment.define(
        "children".to_string(),
        RuntimeValue::Data(Value::array(Vec::new())),
    );
    let Flow::Value(RuntimeValue::Data(Value::NominalRecord {
        record_id, fields, ..
    })) = machine
        .eval(0, environment)
        .expect("generic record literal")
    else {
        panic!("generic record literal was not nominal");
    };
    assert_eq!(record_id.as_ref(), "std.dom::Element");
    assert_eq!(fields[0].0.as_ref(), "tag");
    assert_eq!(fields[1].0.as_ref(), "children");
}

#[test]
fn nominal_record_literal_evaluates_an_omitted_field_default() {
    let default_id = "test:expression/boolean:false";
    let facts = format!(
        r#"{{
            "schema":"topaz.self-target-adapter-facts/v1",
            "nominals":[
                {{"name":"StudyTask","identity":"sample::StudyTask","kind":"record","members":[
                    {{"name":"title","arity":1,"defaultOperationId":null}},
                    {{"name":"done","arity":1,"defaultOperationId":"{default_id}"}}
                ]}}
            ],
            "operationNominals":[]
        }}"#
    );
    let mut record = operation("expression/record-update", "");
    record.operands = vec![1, 2];
    record.operand_labels = vec![
        "base:0".to_string(),
        "field-initializer[0]=title/fields:0/value:0".to_string(),
    ];
    let base = operation("expression/identifier", "StudyTask");
    let title = operation("expression/string-text", "Build application");
    let default = operation("expression/boolean", "false");
    assert_eq!(default.id, default_id);
    let mut machine = Machine::new_with_facts(
        Arc::new(Program {
            modules: Vec::new(),
            operations: vec![record, base, title, default],
        }),
        Some(&facts),
    )
    .expect("target facts with record default");
    let Flow::Value(RuntimeValue::Data(Value::NominalRecord {
        record_id, fields, ..
    })) = machine
        .eval(0, EnvironmentFrame::root())
        .expect("record default")
    else {
        panic!("record default did not produce a nominal record");
    };
    assert_eq!(record_id.as_ref(), "sample::StudyTask");
    assert!(matches!(&fields[1], (name, Value::Bool(false)) if name.as_ref() == "done"));
}

#[test]
fn integer_range_pattern_preserves_its_inclusive_boundary() {
    let mut range = operation("pattern/range", "true");
    range.operands = vec![1, 2];
    let lo = operation("expression/integer", "1");
    let hi = operation("expression/integer", "15");
    let mut machine = Machine::new(Arc::new(Program {
        modules: Vec::new(),
        operations: vec![range, lo, hi],
    }));
    assert!(
        run_local(machine.match_pattern(
            0,
            RuntimeValue::Data(Value::Int(15)),
            EnvironmentFrame::root(),
        ))
        .expect("inclusive range")
    );
    machine.program = Arc::new(Program {
        modules: Vec::new(),
        operations: {
            let mut range = operation("pattern/range", "false");
            range.operands = vec![1, 2];
            vec![
                range,
                operation("expression/integer", "1"),
                operation("expression/integer", "15"),
            ]
        },
    });
    assert!(
        !run_local(machine.match_pattern(
            0,
            RuntimeValue::Data(Value::Int(15)),
            EnvironmentFrame::root(),
        ))
        .expect("exclusive range")
    );
}

#[test]
fn block_defers_run_in_lifo_order() {
    let mut block = operation("expression/block", "");
    block.operands = vec![1, 2];
    let mut first_defer = operation("defer", "");
    first_defer.operands = vec![3];
    let mut second_defer = operation("defer", "");
    second_defer.operands = vec![6];
    let mut first_assignment = operation("assignment", "assign");
    first_assignment.operands = vec![4, 5];
    let first_target = operation("expression/identifier", "value");
    let first_value = operation("expression/integer", "1");
    let mut second_assignment = operation("assignment", "assign");
    second_assignment.operands = vec![7, 8];
    let second_target = operation("expression/identifier", "value");
    let second_value = operation("expression/integer", "2");
    let mut machine = Machine::new(Arc::new(Program {
        modules: Vec::new(),
        operations: vec![
            block,
            first_defer,
            second_defer,
            first_assignment,
            first_target,
            first_value,
            second_assignment,
            second_target,
            second_value,
        ],
    }));
    let environment = EnvironmentFrame::root();
    environment.define("value".to_string(), RuntimeValue::Data(Value::Int(0)));
    machine
        .eval(0, environment.clone())
        .expect("block with defers");
    let RuntimeValue::Data(Value::Int(value)) = environment
        .slot("value")
        .expect("value slot")
        .borrow()
        .clone()
    else {
        panic!("defer assignment did not preserve int");
    };
    assert_eq!(value, 1);
}

#[test]
fn function_call_evaluates_an_omitted_trailing_default() {
    let mut function = operation("function", "");
    function.binding_name = "enabled".to_string();
    function.operands = vec![1, 3];
    let mut parameter = operation("binding/parameter", "enabled");
    parameter.binding_name = "enabled".to_string();
    parameter.operands = vec![2];
    let default = operation("expression/boolean", "false");
    let mut body = operation("expression/block", "");
    body.operands = vec![4];
    let value = operation("expression/identifier", "enabled");
    let mut machine = Machine::new(Arc::new(Program {
        modules: Vec::new(),
        operations: vec![function, parameter, default, body, value],
    }));
    let RuntimeValue::Data(Value::Bool(value)) = machine
        .call_function(0, Vec::new())
        .expect("function parameter default")
    else {
        panic!("function default did not return bool");
    };
    assert!(!value);
}

#[test]
fn compound_assignment_uses_the_preserved_operator() {
    let operation = operation("assignment", "add");
    let RuntimeValue::Data(Value::Int(value)) = assignment_value(
        "add",
        Some(RuntimeValue::Data(Value::Int(40))),
        RuntimeValue::Data(Value::Int(2)),
        &operation,
    )
    .expect("compound assignment") else {
        panic!("compound assignment did not return int");
    };
    assert_eq!(value, 42);
}

#[test]
fn product_runtime_reaches_the_shared_recursion_guard_on_its_owned_stack() {
    let call_span = Span::new(FileId(0), 7, 12);
    let diagnostic = run_on_self_runtime_stack("product recursion guard", || {
        let mut function = operation("function", "recursive");
        function.binding_name = "recursive".to_string();
        function.operands = vec![1];
        let mut body = operation("expression/block", "");
        body.operands = vec![2];
        let mut call = operation("expression/call", "");
        call.lo = call_span.lo;
        call.hi = call_span.hi;
        call.operands = vec![3];
        call.call_target = "test::recursive".to_string();
        call.call_callee_kind = "value".to_string();
        call.call_evaluations = vec![CallEvaluation::Callee];
        let mut callee = operation("expression/identifier", "recursive");
        callee.reference_identity = "test::recursive".to_string();
        let program = Program {
            modules: Vec::new(),
            operations: vec![function, body, call, callee],
        };
        let mut machine = Machine::new(Arc::new(program));
        machine.register_functions()?;
        let error = match machine.call_function(0, Vec::new()) {
            Ok(_) => return Err("call-depth limit was not enforced".to_string()),
            Err(error) => error,
        };
        decode_runtime_diagnostic(&error)
            .ok_or_else(|| "call-depth failure was not a runtime diagnostic".to_string())
    })
    .expect("product recursion guard");
    assert_eq!(
        (diagnostic.code, diagnostic.message, diagnostic.span),
        (
            "TPZ5009",
            "call depth exceeded the recursion limit of 1000".to_string(),
            call_span,
        )
    );
}

#[test]
fn builtin_input_uses_the_invocation_payload() {
    let mut input = operation("expression/call", "input");
    input.call_target = "builtin::input".to_string();
    let program = Program {
        modules: Vec::new(),
        operations: vec![input.clone()],
    };
    let mut machine = Machine::new_with_facts_and_input(Arc::new(program), None, "2 + 3 * 4\n")
        .expect("runtime input");
    let Flow::Value(RuntimeValue::Data(Value::Str(value))) = machine
        .call_builtin(&input, Vec::new())
        .expect("input builtin")
    else {
        panic!("input builtin did not return a string");
    };
    assert_eq!(value.as_ref(), "2 + 3 * 4\n");
}

#[test]
fn regex_compile_uses_the_shared_value_leaf() {
    let mut compile = operation("expression/call", "Regex.compile");
    compile.call_method = "compile".to_string();
    let program = Program {
        modules: Vec::new(),
        operations: Vec::new(),
    };
    let machine = Machine::new(Arc::new(program));
    let Flow::Value(RuntimeValue::Data(Value::Ok(value))) = machine
        .call_static(&compile, "Regex", vec![Value::str("[0-9]")])
        .expect("Regex.compile")
    else {
        panic!("Regex.compile did not return Ok");
    };
    assert!(matches!(&*value, Value::Regex(_)));
}
