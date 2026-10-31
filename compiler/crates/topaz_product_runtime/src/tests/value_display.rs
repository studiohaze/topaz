use super::*;

#[test]
fn tagged_templates_preserve_parts_values_and_path_normalization() {
    let mut template = operation("expression/string", "sql|single-line");
    template.operands = vec![1, 2, 3, 4];
    let program = Program::new(
        Vec::new(),
        vec![
            template,
            operation("expression/string-text", "select * from "),
            operation("expression/identifier", "table"),
            operation("expression/string-text", " where active = "),
            operation("expression/boolean", "true"),
        ],
    );
    let mut machine = Machine::new(Arc::new(program));
    let environment = EnvironmentFrame::root();
    environment.define("table".into(), RuntimeValue::Data(Value::str("users")));
    let value = data(run_local(machine.eval_value(0, environment)).expect("template evaluation"))
        .expect("template value");
    assert_eq!(
        topaz_value::value::render(&value),
        "<sql template, 3 part(s), 2 interpolation(s)>"
    );
    assert_eq!(
        topaz_value::value::render(
            &topaz_value::member_value_required(
                &value,
                "tag",
                span(&operation("expression/member", "tag"))
            )
            .expect("template tag")
        ),
        "sql"
    );
    assert_eq!(
        topaz_value::value::render(
            &topaz_value::member_value_required(
                &value,
                "parts",
                span(&operation("expression/member", "parts"))
            )
            .expect("template parts")
        ),
        "[select * from ,  where active = , ]"
    );

    let mut template = operation("expression/string", "p|single-line");
    template.operands = vec![1, 2];
    let program = Program::new(
        Vec::new(),
        vec![
            template,
            operation("expression/identifier", "directory"),
            operation("expression/string-text", "/main.tpz"),
        ],
    );
    let mut machine = Machine::new(Arc::new(program));
    let environment = EnvironmentFrame::root();
    environment.define(
        "directory".into(),
        RuntimeValue::Data(Value::str("src\\app")),
    );
    let value = data(run_local(machine.eval_value(0, environment)).expect("path evaluation"))
        .expect("path value");
    assert_eq!(topaz_value::value::render(&value), "src/app/main.tpz");
}

#[test]
fn nominal_display_names_preserve_distinct_module_identities() {
    let facts = r#"{
      "schema":"topaz.self-target-adapter-facts/v1",
      "nominals":[
        {"name":"Token","identity":"left::Token","kind":"record","members":[{"name":"value","arity":1}]},
        {"name":"Token","identity":"right::Token","kind":"record","members":[{"name":"value","arity":1}]},
        {"name":"Message","identity":"left::Message","kind":"enum","members":[{"name":"Empty","arity":0},{"name":"Text","arity":1}]},
        {"name":"UserId","identity":"left::UserId","kind":"newtype","members":[]}
      ],
      "operationNominals":[{"operationId":"record-literal","identity":"left::Token","kind":"record"},{"operationId":"right-literal","identity":"right::Token","kind":"record"}]
    }"#;
    let mut record = operation("expression/record-literal", "");
    record.id = "record-literal".into();
    record.operands = vec![2];
    record.operand_labels = vec!["field-initializer[0]=value".into()];
    let mut right_record = record.clone();
    right_record.id = "right-literal".into();
    let program = Program::new(
        Vec::new(),
        vec![record, right_record, operation("expression/integer", "7")],
    );
    let mut machine =
        Machine::new_with_facts(Arc::new(program), Some(facts)).expect("nominal facts");
    let left =
        data(run_local(machine.eval_value(0, EnvironmentFrame::root())).expect("record literal"))
            .expect("record value");
    assert_eq!(topaz_value::value::render(&left), "Token { value: 7 }");
    assert_eq!(left.nominal_declaration_id(), Some("left::Token"));
    let call = operation("expression/call", "");
    let right =
        data(run_local(machine.eval_value(1, EnvironmentFrame::root())).expect("record literal"))
            .expect("record value");
    assert_eq!(topaz_value::value::render(&right), "Token { value: 7 }");
    assert!(!topaz_value::value::values_equal(&left, &right).expect("record equality"));
    let Flow::Value(value) = machine
        .construct_enum(&call, "left::Message", "Text", vec![Value::str("🙂")])
        .expect("enum payload")
    else {
        panic!("enum result")
    };
    assert_eq!(
        topaz_value::value::render(&data(value).expect("enum value")),
        "Message.Text(🙂)"
    );
    let Flow::Value(value) = machine
        .eval_nominal_member(&operation("expression/member", "Empty"), "left::Message")
        .expect("nullary variant")
    else {
        panic!("enum result")
    };
    assert_eq!(
        topaz_value::value::render(&data(value).expect("enum value")),
        "Message.Empty"
    );
    let Flow::Value(value) = machine
        .construct_nominal(
            &call,
            "left::UserId",
            vec![RuntimeValue::Data(Value::Int(7))],
        )
        .expect("newtype")
    else {
        panic!("newtype result")
    };
    assert_eq!(
        topaz_value::value::render(&data(value).expect("newtype value")),
        "UserId(7)"
    );
}

#[test]
fn function_values_keep_declared_names_and_anonymous_lambdas() {
    let mut function = operation("function", "increment");
    function.binding_name = "increment".into();
    function.declaration_identity = "test::increment".into();
    function.operands = vec![3];
    let mut identifier = operation("expression/identifier", "increment");
    identifier.reference_identity = "test::increment".into();
    let mut lambda = operation("expression/lambda", "");
    lambda.operands = vec![3];
    let mut body = operation("expression/block", "");
    body.operands = vec![4];
    let program = Program::new(
        Vec::new(),
        vec![
            function,
            identifier,
            lambda,
            body,
            operation("expression/integer", "1"),
        ],
    );
    let mut machine = Machine::new(Arc::new(program));
    machine.register_functions().expect("function registration");
    let environment = EnvironmentFrame::root();
    let value =
        data(run_local(machine.eval_value(1, environment.clone())).expect("named function"))
            .expect("function value");
    assert_eq!(topaz_value::value::render(&value), "<function increment>");
    let value =
        data(run_local(machine.eval_value(2, environment)).expect("lambda")).expect("lambda value");
    assert_eq!(topaz_value::value::render(&value), "<function>");
}
