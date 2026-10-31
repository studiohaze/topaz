use super::*;

#[test]
fn local_const_function_alias_releases_call_frames() {
    // f() { function g() { 0 }; const G = g; 0 }
    // Each leaked activation owns its parent, so the parent's strong count
    // detects retained call frames without RSS or allocator timing noise.
    let mut g = operation("function", "g");
    g.binding_name = "g".into();
    g.declaration_identity = "test::f::g".into();
    g.operands = vec![0];
    g.operand_labels = vec!["body:0".into()];
    let mut reference = operation("expression/identifier", "g");
    reference.reference_identity = g.declaration_identity.clone();
    let mut alias = operation("constant", "G");
    alias.binding_name = "G".into();
    alias.declaration_identity = "test::f::G".into();
    alias.operands = vec![2];
    let mut body = operation("expression/block", "");
    body.operands = vec![1, 3, 0];
    let mut f = operation("function", "f");
    f.binding_name = "f".into();
    f.operands = vec![4];
    f.operand_labels = vec!["body:0".into()];
    let program = Program::new(
        Vec::new(),
        vec![
            operation("expression/integer", "0"),
            g,
            reference,
            alias,
            body,
            f,
        ],
    );
    let mut machine = Machine::new(Arc::new(program));
    let parent = EnvironmentFrame::root();
    let mut owners = Vec::new();
    for count in 1..=100_000 {
        let result = run_local(machine.call_function_with_environment_async(
            5,
            parent.clone(),
            EvaluatedCallArguments::positional(Vec::new()),
            Span::new(FileId(0), 0, 1),
        ))
        .unwrap();
        assert!(matches!(result, RuntimeValue::Data(Value::Int(0))));
        if count == 1_000 || count == 100_000 {
            owners.push((count, Rc::strong_count(&parent)));
        }
    }
    eprintln!("parent strong counts (calls, owners): {owners:?}");
    assert_eq!(owners, [(1_000, 1), (100_000, 1)]);
}

// Exercise both call encodings, including a static target that must yield to
// the local declaration's execution binding.
fn local_function_call(planned: bool, outer: bool, after: bool) -> Result<i64, String> {
    let mut outer_function = operation("function", "later");
    outer_function.binding_name = "later".into();
    outer_function.declaration_identity = "test::later".into();
    outer_function.operands = vec![0];
    outer_function.operand_labels = vec!["body:0".into()];
    let mut inner_function = operation("function", "later");
    inner_function.binding_name = "later".into();
    inner_function.declaration_identity = "test::local::later".into();
    inner_function.operands = vec![1];
    inner_function.operand_labels = vec!["body:0".into()];
    let mut callee = operation("expression/identifier", "later");
    callee.reference_identity = "test::local::later".into();
    let mut call = operation("expression/call", "");
    call.operands = vec![4];
    call.call_target = "test::later".into();
    call.call_callee_kind = "value".into();
    if planned {
        call.call_evaluations = vec![CallEvaluation::Callee];
    }
    let mut returned = operation("return", "");
    returned.operands = vec![5];
    let mut block = operation("expression/block", "");
    block.operands = if after { vec![3, 6] } else { vec![6, 3] };
    let program = Program::new(
        Vec::new(),
        vec![
            operation("expression/integer", "0"),
            operation("expression/integer", "1"),
            outer_function,
            inner_function,
            callee,
            call,
            returned,
            block,
        ],
    );
    let mut machine = Machine::new(Arc::new(program));
    machine.register_functions()?;
    let environment = EnvironmentFrame::root();
    if outer {
        machine.eval(2, environment.clone())?;
    }
    match machine.eval(7, environment)? {
        Flow::Return(RuntimeValue::Data(Value::Int(value))) => Ok(value),
        _ => Err("expected returned int".into()),
    }
}

#[test]
fn local_function_publication_controls_planned_and_static_calls() {
    for planned in [false, true] {
        assert_eq!(local_function_call(planned, true, false).unwrap(), 0);
        assert_eq!(local_function_call(planned, true, true).unwrap(), 1);
        assert_eq!(local_function_call(planned, false, true).unwrap(), 1);
    }
}

#[test]
fn unpublished_local_function_blocks_static_global_fallback() {
    for planned in [false, true] {
        let error = local_function_call(planned, false, false).unwrap_err();
        assert_eq!(
            decode_runtime_diagnostic(&error).expect(&error).code,
            "TPZ5002"
        );
    }
}

#[test]
fn typed_json_checks_target_and_preserves_nominal_identity() {
    use crate::runtime::nominal::{NominalFact, NominalMemberFact, NominalRegistry};
    let mut machine = Machine::new(Arc::new(Program::new(Vec::new(), Vec::new())));
    let mut nominals = NominalRegistry::default();
    nominals.by_identity.insert(
        "test::Book".into(),
        NominalFact {
            name: "Book".into(),
            identity: "test::Book".into(),
            kind: "record".into(),
            type_parameters: vec![],
            base_type: None,
            members: vec![NominalMemberFact {
                name: "pages".into(),
                arity: 1,
                default_operation_id: None,
                types: vec![SemanticType::Primitive(SemanticPrimitive::Int)],
            }],
        },
    );
    machine.nominals = Arc::new(nominals);
    for (ty, input, expected) in [
        (
            SemanticType::NominalRecord {
                identity: "test::Book".into(),
                arguments: vec![],
            },
            r#"{"pages":"many"}"#,
            "Err($.pages: expected int, found string)",
        ),
        (
            SemanticType::NominalRecord {
                identity: "test::Book".into(),
                arguments: vec![],
            },
            "{}",
            "Err($: missing required field `pages`)",
        ),
        (
            SemanticType::NominalRecord {
                identity: "test::Book".into(),
                arguments: vec![],
            },
            r#"{"pages":7}"#,
            "Ok(Book { pages: 7 })",
        ),
        (
            SemanticType::Constructor {
                constructor: SemanticConstructor::Array,
                arguments: vec![SemanticType::Primitive(SemanticPrimitive::Int)],
            },
            r#"[1,"2"]"#,
            "Err($[1]: expected int, found string)",
        ),
    ] {
        let mut call = operation("expression/call", "");
        call.call_type_arguments = vec![ty];
        for kind in [Builtin::JsonParseAs, Builtin::JsonDecode] {
            let (recv, args) = if kind == Builtin::JsonParseAs {
                (None, vec![RuntimeValue::Data(Value::str(input))])
            } else {
                (
                    Some(Rc::new(Value::Json(
                        topaz_value::value::json_parse(input).unwrap().into(),
                    ))),
                    vec![],
                )
            };
            let actual = run_local(machine.call_builtin_value(
                kind,
                recv,
                EvaluatedCallArguments::positional(args),
                &call,
            ))
            .unwrap();
            assert_eq!(
                topaz_value::value::render(&actual),
                expected,
                "{kind:?}: {input}"
            );
        }
    }
}

// These facts mirror the self compiler's nominal table: local member types
// are bare, imported member types are qualified.
fn json_record_type(identity: &str) -> SemanticType {
    SemanticType::NominalRecord {
        identity: identity.into(),
        arguments: vec![],
    }
}

fn json_record_fact(
    identity: &str,
    fields: Vec<(&str, SemanticType)>,
) -> crate::runtime::nominal::NominalFact {
    use crate::runtime::nominal::{NominalFact, NominalMemberFact};
    NominalFact {
        name: identity.rsplit("::").next().unwrap().into(),
        identity: identity.into(),
        kind: "record".into(),
        type_parameters: vec![],
        base_type: None,
        members: fields
            .into_iter()
            .map(|(name, ty)| NominalMemberFact {
                name: name.into(),
                arity: 1,
                default_operation_id: None,
                types: vec![ty],
            })
            .collect(),
    }
}

fn json_nominal_machine(facts: Vec<crate::runtime::nominal::NominalFact>) -> Machine {
    let mut machine = Machine::new(Arc::new(Program::new(Vec::new(), Vec::new())));
    let mut nominals = crate::runtime::nominal::NominalRegistry::default();
    for fact in facts {
        nominals.by_identity.insert(fact.identity.clone(), fact);
    }
    machine.nominals = Arc::new(nominals);
    machine
}

fn decode_json_target(
    machine: &mut Machine,
    kind: Builtin,
    module: &str,
    ty: SemanticType,
    input: &str,
) -> Result<Value, String> {
    let mut call = operation("expression/call", "");
    call.module = module.into();
    call.call_type_arguments = vec![ty];
    let (recv, args) = if kind == Builtin::JsonParseAs {
        (None, vec![RuntimeValue::Data(Value::str(input))])
    } else {
        (
            Some(Rc::new(Value::Json(
                topaz_value::value::json_parse(input).unwrap().into(),
            ))),
            vec![],
        )
    };
    run_local(machine.call_builtin_value(
        kind,
        recv,
        EvaluatedCallArguments::positional(args),
        &call,
    ))
}

#[test]
fn typed_json_resolves_same_module_nested_records() {
    let mut machine = json_nominal_machine(vec![
        json_record_fact(
            "app::main::Entry",
            vec![("name", SemanticType::Primitive(SemanticPrimitive::String))],
        ),
        json_record_fact(
            "app::main::Listing",
            vec![(
                "entries",
                SemanticType::Constructor {
                    constructor: SemanticConstructor::Array,
                    arguments: vec![json_record_type("Entry")],
                },
            )],
        ),
    ]);
    for kind in [Builtin::JsonParseAs, Builtin::JsonDecode] {
        for identity in ["app::main::Listing", "Listing"] {
            let actual = decode_json_target(
                &mut machine,
                kind,
                "app::main",
                json_record_type(identity),
                r#"{"entries":[{"name":"a"},{"name":"b"}]}"#,
            )
            .unwrap();
            assert_eq!(
                topaz_value::value::render(&actual),
                "Ok(Listing { entries: [Entry { name: a }, Entry { name: b }] })"
            );
            let Value::Ok(listing) = actual else {
                panic!("expected decoded listing")
            };
            let Some(Value::Array(entries)) = listing.nominal_field("entries") else {
                panic!("expected entries array")
            };
            let entries = entries.borrow();
            assert_eq!(entries.len(), 2);
            let Value::NominalRecord {
                declaration_identity,
                ..
            } = &entries[0]
            else {
                panic!("expected nominal entry")
            };
            assert_eq!(declaration_identity.as_deref(), Some("app::main::Entry"));
        }
    }
}

#[test]
fn typed_json_preserves_imported_and_generic_argument_modules() {
    let mut imported_box = json_record_fact(
        "dep::Box",
        vec![(
            "value",
            SemanticType::Rigid {
                name: "T".into(),
                _origin: String::new(),
            },
        )],
    );
    imported_box.type_parameters = vec!["T".into()];
    let mut machine = json_nominal_machine(vec![
        json_record_fact(
            "test::Entry",
            vec![("name", SemanticType::Primitive(SemanticPrimitive::String))],
        ),
        json_record_fact(
            "dep::Entry",
            vec![("number", SemanticType::Primitive(SemanticPrimitive::Int))],
        ),
        json_record_fact("dep::Imported", vec![("entry", json_record_type("Entry"))]),
        imported_box,
        json_record_fact(
            "test::Mixed",
            vec![
                ("local", json_record_type("Entry")),
                ("imported", json_record_type("dep::Imported")),
                (
                    "boxed",
                    SemanticType::NominalRecord {
                        identity: "dep::Box".into(),
                        arguments: vec![json_record_type("Entry")],
                    },
                ),
            ],
        ),
    ]);
    for kind in [Builtin::JsonParseAs, Builtin::JsonDecode] {
        let actual = decode_json_target(&mut machine, kind, "test", json_record_type("Mixed"), r#"{"local":{"name":"a"},"imported":{"entry":{"number":7}},"boxed":{"value":{"name":"b"}}}"#).unwrap();
        assert_eq!(
            topaz_value::value::render(&actual),
            "Ok(Mixed { local: Entry { name: a }, imported: Imported { entry: Entry { number: 7 } }, boxed: Box { value: Entry { name: b } } })"
        );
        let actual = decode_json_target(
            &mut machine,
            kind,
            "test",
            SemanticType::NominalRecord {
                identity: "dep::Box".into(),
                arguments: vec![json_record_type("Entry")],
            },
            r#"{"value":{"name":"c"}}"#,
        )
        .unwrap();
        assert_eq!(
            topaz_value::value::render(&actual),
            "Ok(Box { value: Entry { name: c } })"
        );
        let actual = decode_json_target(
            &mut machine,
            kind,
            "test",
            SemanticType::NominalRecord {
                identity: "dep::Box".into(),
                arguments: vec![SemanticType::NominalRecord {
                    identity: "dep::Box".into(),
                    arguments: vec![json_record_type("Entry")],
                }],
            },
            r#"{"value":{"value":{"name":"d"}}}"#,
        )
        .unwrap();
        assert_eq!(
            topaz_value::value::render(&actual),
            "Ok(Box { value: Box { value: Entry { name: d } } })"
        );
    }
}

#[test]
fn typed_json_resolves_newtype_and_enum_payloads() {
    let mut wrapper = json_record_fact("dep::Wrapper", vec![]);
    wrapper.kind = "newtype".into();
    wrapper.base_type = Some(json_record_type("Entry"));
    let mut choice = json_record_fact(
        "dep::Choice",
        vec![(
            "Item",
            SemanticType::Newtype {
                identity: "Wrapper".into(),
                arguments: vec![],
            },
        )],
    );
    choice.kind = "enum".into();
    let mut machine = json_nominal_machine(vec![
        json_record_fact(
            "dep::Entry",
            vec![("name", SemanticType::Primitive(SemanticPrimitive::String))],
        ),
        wrapper,
        choice,
    ]);
    for kind in [Builtin::JsonParseAs, Builtin::JsonDecode] {
        let actual = decode_json_target(
            &mut machine,
            kind,
            "test",
            SemanticType::Enum {
                identity: "dep::Choice".into(),
                arguments: vec![],
            },
            r#"{"tag":"Item","values":[{"name":"a"}]}"#,
        )
        .unwrap();
        assert_eq!(
            topaz_value::value::render(&actual),
            "Ok(Choice.Item(Wrapper(Entry { name: a })))"
        );
    }
}

#[test]
fn typed_json_missing_nominal_and_recursive_errors_are_unchanged() {
    let mut machine = json_nominal_machine(vec![
        json_record_fact("other::Missing", vec![]),
        {
            let mut fact = json_record_fact(
                "other::Box",
                vec![(
                    "value",
                    SemanticType::Rigid {
                        name: "T".into(),
                        _origin: String::new(),
                    },
                )],
            );
            fact.type_parameters = vec!["T".into()];
            fact
        },
        json_record_fact("test::Broken", vec![("entry", json_record_type("Missing"))]),
        json_record_fact(
            "test::Recursive",
            vec![("next", json_record_type("Recursive"))],
        ),
    ]);
    for kind in [Builtin::JsonParseAs, Builtin::JsonDecode] {
        for module in ["test", ""] {
            assert_eq!(
                decode_json_target(
                    &mut machine,
                    kind,
                    module,
                    SemanticType::NominalRecord {
                        identity: "other::Box".into(),
                        arguments: vec![json_record_type("Missing")],
                    },
                    "{}"
                )
                .unwrap_err(),
                "missing JSON nominal `Missing`"
            );
        }
        for (module, target, expected) in [
            ("test", "Broken", "missing JSON nominal `Missing`"),
            ("test", "Missing", "missing JSON nominal `Missing`"),
            ("", "Missing", "missing JSON nominal `Missing`"),
            (
                "test",
                "other::Box",
                "JSON nominal `other::Box` has inconsistent type arguments",
            ),
            (
                "test",
                "absent::Missing",
                "missing JSON nominal `absent::Missing`",
            ),
            (
                "test",
                "Recursive",
                "JSON_LIMIT: a recursive type `Recursive` has no finite JSON decode schema",
            ),
        ] {
            assert_eq!(
                decode_json_target(&mut machine, kind, module, json_record_type(target), "{}")
                    .unwrap_err(),
                expected
            );
        }
    }
}

#[test]
fn typed_json_keeps_the_128_nominal_schema_limit() {
    let mut machine = json_nominal_machine(
        (0..129)
            .map(|index| {
                json_record_fact(
                    &format!("test::N{index}"),
                    if index == 128 {
                        vec![]
                    } else {
                        vec![("next", json_record_type(&format!("N{}", index + 1)))]
                    },
                )
            })
            .collect(),
    );
    let mut call = operation("expression/call", "");
    call.call_type_arguments = vec![json_record_type("N1")];
    run_local(machine.call_json_schema(&call)).expect("128 nominal schemas are supported");
    call.call_type_arguments = vec![json_record_type("N0")];
    assert_eq!(
        run_local(machine.call_json_schema(&call)).unwrap_err(),
        "JSON_LIMIT: a recursive type `N128` has no finite JSON decode schema"
    );
}

#[test]
fn json_decode_method_value_dispatch_preserves_the_bound_receiver() {
    for planned in [false, true] {
        let callee = operation("expression/identifier", "decode");
        let mut call = operation("expression/call", "");
        call.operands = vec![0];
        call.operand_labels = vec!["callee:0".into()];
        call.call_target = "builtin::JsonDecode".into();
        call.call_callee_kind = "value".into();
        call.call_type_arguments = vec![SemanticType::Primitive(SemanticPrimitive::Int)];
        if planned {
            call.call_evaluations = vec![CallEvaluation::Callee];
        }
        let mut machine = Machine::new(Arc::new(Program::new(Vec::new(), vec![callee, call])));
        let environment = EnvironmentFrame::root();
        environment.define(
            "decode".into(),
            RuntimeValue::Data(Value::Builtin {
                kind: Builtin::JsonDecode,
                recv: Some(Rc::new(Value::Json(
                    topaz_value::value::json_parse("9").unwrap().into(),
                ))),
            }),
        );
        let actual = data(run_local(machine.eval_value(1, environment)).unwrap()).unwrap();
        assert_eq!(
            topaz_value::value::render(&actual),
            "Ok(9)",
            "planned={planned}"
        );
    }
}

#[test]
fn static_catalog_calls_execute_documented_values() {
    let mut machine = Machine::new(Arc::new(Program::new(Vec::new(), Vec::new())));
    for (namespace, member, arguments, expected) in [
        (
            "Hash",
            "crc32",
            vec![Value::Bytes(Rc::from(*b"123456789"))],
            "3421780262",
        ),
        ("Array", "of", vec![Value::Int(2), Value::Int(3)], "[2, 3]"),
        ("Math", "sqrt", vec![Value::Float(9.0)], "Ok(3.0)"),
    ] {
        let mut call = operation("expression/call", member);
        call.call_method = member.to_string();
        let Flow::Value(RuntimeValue::Data(value)) =
            run_local(machine.call_static(&call, namespace, arguments))
                .unwrap_or_else(|error| panic!("{namespace}.{member}: {error}"))
        else {
            panic!("{namespace}.{member} did not return data");
        };
        assert_eq!(
            topaz_value::value::render(&value),
            expected,
            "{namespace}.{member}"
        );
    }
}

#[test]
fn rounding_mode_members_are_standard_library_values() {
    let machine = Machine::new(Arc::new(Program::new(Vec::new(), Vec::new())));
    let member = operation("expression/member", "HalfEven");
    let Flow::Value(RuntimeValue::Data(value)) = machine
        .eval_nominal_member(&member, "RoundingMode")
        .expect("standard rounding mode")
    else {
        panic!("rounding mode did not return data");
    };
    assert_eq!(topaz_value::value::render(&value), "RoundingMode.HalfEven");
}

#[test]
fn module_evaluation_returns_the_entry_value_instead_of_another_modules_value() {
    let mut entry = operation("module", "");
    entry.operands = vec![1];
    let mut dependency = operation("module", "dependency");
    dependency.module = "dependency".to_string();
    dependency.operands = vec![3];
    let mut dependency_value = operation("expression/integer", "99");
    dependency_value.module = "dependency".to_string();
    let program = Program::new(
        vec![
            Module {
                generated_std: false,
                identity: "test".to_string(),
                source_ordinal: 0,
                entry: true,
                operations: vec![0, 1],
            },
            Module {
                generated_std: false,
                identity: "dependency".to_string(),
                source_ordinal: 1,
                entry: false,
                operations: vec![2, 3],
            },
        ],
        vec![
            entry,
            operation("expression/integer", "42"),
            dependency,
            dependency_value,
        ],
    );
    let mut machine = Machine::new(Arc::new(program));
    let value =
        data(machine.initialize_modules().expect("module initialization")).expect("entry value");
    assert!(matches!(value, Value::Int(42)));
}

#[test]
fn lexical_slot_precedes_colliding_module_function() {
    let mut identifier = operation("expression/identifier", "children");
    identifier.reference_identity = "checker::children".to_string();
    let program = Program::new(Vec::new(), vec![identifier.clone()]);
    let mut machine = Machine::new(Arc::new(program));
    Rc::get_mut(&mut machine.functions)
        .expect("new machine owns its function table")
        .insert("checker::children".to_string(), 0);
    let environment = EnvironmentFrame::root();
    environment.define("children".to_string(), RuntimeValue::Data(Value::Int(42)));
    let Flow::Value(RuntimeValue::Data(Value::Int(value))) = machine
        .eval_identifier(0, &identifier, environment)
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
    let program = Program::new(Vec::new(), vec![optional, identifier]);
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
    let mut machine = Machine::new(Arc::new(Program::new(Vec::new(), Vec::new())));
    let Flow::Value(RuntimeValue::Data(Value::Map(map))) =
        run_local(machine.call_static(&call, "Map", Vec::new())).expect("Map.new")
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
    let mut machine = Machine::new(Arc::new(Program::new(
        Vec::new(),
        vec![
            comprehension,
            operation("expression/integer", "1"),
            operation("expression/integer", "2"),
        ],
    )));
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
    let program = Program::new(Vec::new(), vec![pattern, binding]);
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
            {"name":"Element","identity":"view::Element","kind":"record","members":[
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
        Arc::new(Program::new(Vec::new(), vec![record, base, tag, children])),
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
        record_id,
        declaration_identity,
        fields,
        ..
    })) = machine
        .eval(0, environment)
        .expect("generic record literal")
    else {
        panic!("generic record literal was not nominal");
    };
    assert_eq!(
        nominal_declaration_identity(&record_id, declaration_identity.as_deref()),
        "view::Element"
    );
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
        Arc::new(Program::new(Vec::new(), vec![record, base, title, default])),
        Some(&facts),
    )
    .expect("target facts with record default");
    let Flow::Value(RuntimeValue::Data(Value::NominalRecord {
        record_id,
        declaration_identity,
        fields,
        ..
    })) = machine
        .eval(0, EnvironmentFrame::root())
        .expect("record default")
    else {
        panic!("record default did not produce a nominal record");
    };
    assert_eq!(
        nominal_declaration_identity(&record_id, declaration_identity.as_deref()),
        "sample::StudyTask"
    );
    assert!(matches!(&fields[1], (name, Value::Bool(false)) if name.as_ref() == "done"));
}

#[test]
fn integer_range_pattern_preserves_its_inclusive_boundary() {
    let mut range = operation("pattern/range", "true");
    range.operands = vec![1, 2];
    let lo = operation("expression/integer", "1");
    let hi = operation("expression/integer", "15");
    let mut machine = Machine::new(Arc::new(Program::new(Vec::new(), vec![range, lo, hi])));
    assert!(
        run_local(machine.match_pattern(
            0,
            RuntimeValue::Data(Value::Int(15)),
            EnvironmentFrame::root(),
        ))
        .expect("inclusive range")
    );
    machine.program = Arc::new(Program::new(Vec::new(), {
        let mut range = operation("pattern/range", "false");
        range.operands = vec![1, 2];
        vec![
            range,
            operation("expression/integer", "1"),
            operation("expression/integer", "15"),
        ]
    }));
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
    let mut machine = Machine::new(Arc::new(Program::new(
        Vec::new(),
        vec![
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
    )));
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
    let mut machine = Machine::new(Arc::new(Program::new(
        Vec::new(),
        vec![function, parameter, default, body, value],
    )));
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
        let program = Program::new(Vec::new(), vec![function, body, call, callee]);
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
    let program = Program::new(Vec::new(), vec![input.clone()]);
    let mut machine = Machine::new_with_facts_and_input(Arc::new(program), None, "2 + 3 * 4\n")
        .expect("runtime input");
    let Flow::Value(RuntimeValue::Data(Value::Str(value))) = machine
        .call_builtin(&input.call_target.clone(), &input, Vec::new())
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
    let program = Program::new(Vec::new(), Vec::new());
    let mut machine = Machine::new(Arc::new(program));
    let Flow::Value(RuntimeValue::Data(Value::Ok(value))) =
        run_local(machine.call_static(&compile, "Regex", vec![Value::str("[0-9]")]))
            .expect("Regex.compile")
    else {
        panic!("Regex.compile did not return Ok");
    };
    assert!(matches!(&*value, Value::Regex(_)));
}

#[test]
fn r19_product_runtime_executes_bound_numeric_leaves_and_gates_static_math() {
    let mut program = Program::new(Vec::new(), Vec::new());
    program.language_version = topaz_value::LangVersion::V5_21;
    let mut machine = Machine::new(Arc::new(program));
    let call = operation("expression/call", "floor");
    for (kind, receiver, expected) in [
        (Builtin::FloatFloor, Value::Float(3.7), "3"),
        (Builtin::MathAbs, Value::Float(-2.0), "2.0"),
        (Builtin::ToFloat, Value::Int(3), "3.0"),
        (Builtin::MathParseFloat, Value::str(" 2.5 "), "Ok(2.5)"),
    ] {
        let value = run_local(machine.call_builtin_value(
            kind,
            Some(Rc::new(receiver)),
            EvaluatedCallArguments::positional(Vec::new()),
            &call,
        ))
        .unwrap();
        assert_eq!(topaz_value::render(&value), expected);
    }
    let mut method = call.clone();
    method.call_method = "floor".into();
    assert!(run_local(machine.call_static(&method, "Math", vec![Value::Float(3.7)])).is_err());
}

#[test]
fn r19_product_ir_language_mode_controls_the_public_math_surface() {
    for mode in ["5.20", "5.21"] {
        let payload = format!(
            r#"{{"schema":"topaz.compiler.fixed-point-ir-payload/v1","loweredOperations":[],"loweredModules":[{{"identity":"test","entry":true,"operationIds":[],"languageMode":"topaz-{mode}"}}]}}"#
        );
        let parsed =
            crate::program::decode_json::parse_program(&payload, ProgramAdmission::TargetProduct)
                .unwrap();
        let mut machine = Machine::new(Arc::new(parsed.program));
        let mut call = operation("expression/call", "floor");
        call.call_method = "floor".into();
        let value = run_local(machine.call_static(&call, "Math", vec![Value::Float(3.7)]));
        if mode == "5.20" {
            let Flow::Value(RuntimeValue::Data(value)) = value.unwrap() else {
                panic!("data result");
            };
            assert_eq!(topaz_value::render(&value), "3.0");
        } else {
            assert!(value.is_err());
        }
    }
}
