use super::*;

#[test]
fn compound_assignment_reads_the_target_before_running_the_right_side() {
    let mut add = operation("assignment", "add");
    add.operands = vec![1, 2];
    let mut right = operation("expression/block", "");
    right.operands = vec![3, 5];
    let mut replace = operation("assignment", "assign");
    replace.operands = vec![1, 4];
    let mut machine = Machine::new(Arc::new(Program::new(
        Vec::new(),
        vec![
            add,
            operation("expression/identifier", "value"),
            right,
            replace,
            operation("expression/integer", "100"),
            operation("expression/integer", "2"),
        ],
    )));
    let environment = EnvironmentFrame::root();
    environment.define("value".into(), RuntimeValue::Data(Value::Int(10)));
    machine
        .eval(0, environment.clone())
        .expect("compound assignment");
    let value = data(environment.lookup(&["value"]).expect("binding")).expect("value");
    assert_eq!(topaz_value::value::render(&value), "12");
}

#[test]
fn coalescing_assignment_skips_present_values_and_evaluates_fallback_once() {
    for indexed in [false, true] {
        for (initial, fallback, expected, calls) in [
            (
                Value::None,
                Value::Some(Rc::new(Value::Int(7))),
                "Some(7)",
                "1",
            ),
            (
                Value::Some(Rc::new(Value::Int(3))),
                Value::Some(Rc::new(Value::Int(7))),
                "Some(3)",
                "0",
            ),
            (Value::Null, Value::Int(7), "7", "1"),
            (Value::Int(3), Value::Int(7), "3", "0"),
        ] {
            let mut assignment = operation("assignment", "coalesce");
            assignment.operands = vec![1, 2];
            let mut target = operation("expression/identifier", "value");
            if indexed {
                target = operation("expression/index", "");
                target.operands = vec![7, 8];
            }
            let mut right = operation("expression/block", "");
            right.operands = vec![3, 6];
            let mut increment = operation("assignment", "add");
            increment.operands = vec![4, 5];
            let mut machine = Machine::new(Arc::new(Program::new(
                Vec::new(),
                vec![
                    assignment,
                    target,
                    right,
                    increment,
                    operation("expression/identifier", "calls"),
                    operation("expression/integer", "1"),
                    operation("expression/identifier", "fallback"),
                    operation("expression/identifier", "array"),
                    operation("expression/integer", "0"),
                ],
            )));
            let environment = EnvironmentFrame::root();
            let array = Value::array(vec![initial.clone()]);
            environment.define("value".into(), RuntimeValue::Data(initial));
            environment.define("array".into(), RuntimeValue::Data(array.clone()));
            environment.define("fallback".into(), RuntimeValue::Data(fallback));
            environment.define("calls".into(), RuntimeValue::Data(Value::Int(0)));
            machine
                .eval(0, environment.clone())
                .expect("coalescing assignment");
            let value = if indexed {
                let Value::Array(items) = &array else {
                    unreachable!()
                };
                items.borrow()[0].clone()
            } else {
                data(environment.lookup(&["value"]).expect("binding")).expect("binding value")
            };
            assert_eq!(topaz_value::value::render(&value), expected);
            assert_eq!(
                topaz_value::value::render(
                    &data(environment.lookup(&["calls"]).expect("counter")).expect("counter value")
                ),
                calls
            );
        }
    }
}

#[test]
fn coalescing_array_assignment_rechecks_bounds_after_fallback_effects() {
    let mut assignment = operation("assignment", "coalesce");
    assignment.operands = vec![1, 2];
    let mut target = operation("expression/index", "");
    target.operands = vec![3, 4];
    let mut right = operation("expression/block", "");
    right.operands = vec![5, 6];
    let mut pop = operation("expression/call", "");
    pop.operands = vec![7];
    let mut machine = Machine::new(Arc::new(Program::new(
        Vec::new(),
        vec![
            assignment,
            target,
            right,
            operation("expression/identifier", "array"),
            operation("expression/integer", "0"),
            pop,
            operation("expression/identifier", "fallback"),
            operation("expression/identifier", "pop"),
        ],
    )));
    let array = Value::array(vec![Value::None]);
    let pop = topaz_value::bind_receiver_builtin(
        array.clone(),
        "pop",
        span(&operation("expression/member", "pop")),
    )
    .expect("admitted array method");
    let environment = EnvironmentFrame::root();
    environment.define("array".into(), RuntimeValue::Data(array));
    environment.define("pop".into(), RuntimeValue::Data(pop));
    environment.define(
        "fallback".into(),
        RuntimeValue::Data(Value::Some(Rc::new(Value::Int(7)))),
    );
    let error = match machine.eval(0, environment) {
        Err(error) => error,
        Ok(_) => panic!("assignment into the removed array cell succeeded"),
    };
    assert_eq!(
        decode_runtime_diagnostic(&error)
            .expect("array bounds fault")
            .code,
        "TPZ4001"
    );
}
