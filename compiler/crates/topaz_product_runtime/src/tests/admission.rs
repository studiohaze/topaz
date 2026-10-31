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
    let program = Program::new(Vec::new(), Vec::new());
    let machine = Machine::new_with_facts(Arc::new(program), Some(facts)).expect("target facts");
    let member = operation("expression/member", "Reset");
    let Flow::Value(RuntimeValue::Data(Value::Enum {
        enum_id,
        declaration_identity,
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
    assert_eq!(
        nominal_declaration_identity(&enum_id, declaration_identity.as_deref()),
        "src.model::Msg"
    );
    assert_eq!(variant.as_ref(), "Reset");
    assert_eq!(variant_index, 1);
    assert!(payloads.is_empty());

    let Flow::Value(RuntimeValue::Data(Value::NominalRecord {
        record_id,
        declaration_identity,
        fields,
        ..
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
    assert_eq!(
        nominal_declaration_identity(&record_id, declaration_identity.as_deref()),
        "src.model::Model"
    );
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
    let error = parse_embedded_program(b"NOT-C2BIN\x01{}", b"TPZC2BIN", "Stage 2")
        .expect_err("corrupt image must fail closed");
    assert!(error.contains("magic"), "{error}");
}

fn compact_module_image(version: u8, flags: u8) -> Vec<u8> {
    let mut bytes = b"TPZIMAGE".to_vec();
    bytes.push(version);
    if version == 2 {
        compact_u32(&mut bytes, topaz_value::LITERAL_TABLE_MARKER);
        compact_string(&mut bytes, topaz_value::LITERAL_TABLE_ENCODING);
    }
    compact_u32(&mut bytes, 0);
    compact_u32(&mut bytes, 1);
    compact_string(&mut bytes, "std.json");
    bytes.push(flags);
    compact_u32(&mut bytes, 0);
    if version == 2 {
        compact_u32(&mut bytes, 17);
        compact_string(&mut bytes, "topaz-5.21");
    }
    bytes
}

#[test]
fn compact_module_flags_preserve_entry_and_generated_std() {
    for version in [1, 2] {
        for (flags, entry, generated_std) in [
            (0, false, false),
            (1, true, false),
            (2, false, true),
            (3, true, true),
        ] {
            let program =
                parse_embedded_program(&compact_module_image(version, flags), b"TPZIMAGE", "test")
                    .expect("valid module flags");
            assert_eq!(program.modules.len(), 1);
            let module = &program.modules[0];
            assert_eq!(module.identity, "std.json");
            assert_eq!(module.entry, entry, "flags {flags}");
            assert_eq!(module.generated_std, generated_std, "flags {flags}");
            assert_eq!(module.source_ordinal, if version == 2 { 17 } else { 0 });
        }
    }
}

#[test]
fn compact_module_flags_reject_unknown_bits() {
    for version in [1, 2] {
        for flags in 4..=u8::MAX {
            let error =
                parse_embedded_program(&compact_module_image(version, flags), b"TPZIMAGE", "test")
                    .expect_err("unknown module flags must fail closed");
            assert_eq!(error, "Stage 1 compact table module flags are invalid");
        }
    }
}

#[test]
fn compact_image_rejects_unknown_versions() {
    for version in [0].into_iter().chain(3..=u8::MAX) {
        let mut bytes = b"TPZIMAGE".to_vec();
        bytes.push(version);
        let error = parse_embedded_program(&bytes, b"TPZIMAGE", "test").unwrap_err();
        assert!(error.contains("unsupported version"), "{version}: {error}");
    }
}

#[test]
fn compact_boundary_provenance_is_required_in_both_layouts() {
    for version in [1, 2] {
        for target_field in ["referenceIdentity", "callTarget"] {
            for (module, generated_std, admitted) in [
                ("std.json", true, true),
                ("std.json", false, false),
                ("user.json", true, false),
            ] {
                for entry in [false, true] {
                    let mut bytes = b"TPZIMAGE".to_vec();
                    bytes.push(version);
                    if version == 2 {
                        compact_u32(&mut bytes, topaz_value::LITERAL_TABLE_MARKER);
                        compact_string(&mut bytes, topaz_value::LITERAL_TABLE_ENCODING);
                    }
                    compact_u32(&mut bytes, 1);
                    for value in ["intrinsic", module, "expression/identifier", ""] {
                        compact_string(&mut bytes, value);
                    }
                    if version == 2 {
                        compact_string(&mut bytes, "legacy-source/v1");
                    }
                    for field in [
                        "referenceIdentity",
                        "bindingName",
                        "declarationIdentity",
                        "callTarget",
                        "callMethod",
                    ] {
                        compact_string(
                            &mut bytes,
                            if field == target_field {
                                "builtin::__stdJsonParse"
                            } else {
                                ""
                            },
                        );
                    }
                    for value in [0, 1, 0, 0] {
                        compact_u32(&mut bytes, value);
                    }
                    if version == 2 {
                        for _ in 0..3 {
                            compact_string(&mut bytes, "");
                        }
                        bytes.extend_from_slice(&[0, 0]);
                        compact_string(&mut bytes, "");
                        for _ in 0..3 {
                            compact_u32(&mut bytes, 0);
                        }
                        bytes.push(0);
                    }
                    compact_u32(&mut bytes, 1);
                    compact_string(&mut bytes, module);
                    bytes.push(u8::from(entry) | (u8::from(generated_std) << 1));
                    compact_u32(&mut bytes, 1);
                    compact_u32(&mut bytes, 0);
                    if version == 2 {
                        compact_u32(&mut bytes, 13);
                        compact_string(&mut bytes, "topaz-5.21");
                    }
                    let result = parse_embedded_program(&bytes, b"TPZIMAGE", "test");
                    if admitted {
                        result.expect("generated std.json admitted");
                    } else {
                        assert_eq!(
                            result.unwrap_err(),
                            "standard boundary intrinsic requires generated std.json provenance"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn embedded_program_parser_rejects_an_impossible_operation_count_before_allocating() {
    let mut bytes = b"TPZC2BIN\x01".to_vec();
    compact_u32(&mut bytes, u32::MAX);
    let error = parse_embedded_program(&bytes, b"TPZC2BIN", "Stage 2")
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
    let error = parse_embedded_program(&bytes, b"TPZC2BIN", "Stage 2")
        .expect_err("duplicate operation ids must fail closed");
    assert!(error.contains("duplicates operation id `same`"), "{error}");
}

#[test]
fn embedded_program_parser_rejects_an_out_of_range_operand() {
    let bytes = compact_program(&[("root", "expression/integer", &[1])]);
    let error = parse_embedded_program(&bytes, b"TPZC2BIN", "Stage 2")
        .expect_err("out-of-range operand must fail closed");
    assert!(
        error.contains("operation operand is out of range"),
        "{error}"
    );
}

#[test]
fn embedded_program_parser_rejects_a_call_without_a_callee() {
    let bytes = compact_program(&[("call", "expression/call", &[])]);
    let error = parse_embedded_program(&bytes, b"TPZC2BIN", "Stage 2")
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

// --- Untyped JSON decode equivalence corpus -------------------------------
//
// An installed compiler product reaches `JSON.parseAs<T>` and `JSON.decode`
// through `json_to_value`, so a change of meaning here reaches every JSON
// surface the product reads. RECORDED_JSON_VALUES pins the render of each case
// to what the corpus produced before the walk learned to consume the tree it is
// given.

fn json_value_corpus() -> Vec<(&'static str, &'static str)> {
    vec![
        ("null", "null"),
        ("true", "true"),
        ("false", "false"),
        ("string", r#""plain""#),
        ("string-escapes", r#""a\"b\\c\nd\te""#),
        ("string-unicode", r#""é😀""#),
        ("string-empty", r#""""#),
        ("int-zero", "0"),
        ("int-negative-zero", "-0"),
        ("int", "42"),
        ("int-negative", "-7"),
        ("int-max", "9223372036854775807"),
        ("int-min", "-9223372036854775808"),
        ("int-over-i64", "92233720368547758080"),
        ("int-from-exponent", "1e10"),
        ("int-from-trailing-zero-fraction", "1.0"),
        ("float", "1.5"),
        ("float-exponent", "-2.25e-3"),
        ("array-empty", "[]"),
        ("array-ints", "[1,2,3]"),
        ("array-mixed", r#"[null,true,"s",1.5]"#),
        ("array-nested", "[[1],[2,[3]],[]]"),
        ("object-empty", "{}"),
        ("object-key-order", r#"{"b":2,"a":1}"#),
        ("object-nested", r#"{"outer":{"inner":[1,2],"flag":false}}"#),
        ("object-null-member", r#"{"a":null,"b":1}"#),
        (
            "object-wide",
            r#"{"k01":1,"k02":2,"k03":3,"k04":4,"k05":5,"k06":6,"k07":7,"k08":8,"k09":9,"k10":10,"k11":11,"k12":12,"k13":13,"k14":14}"#,
        ),
        (
            "array-of-objects",
            r#"[{"a":1,"b":"x"},{"a":2,"b":"y"},{"a":3,"b":"z"}]"#,
        ),
        (
            "deep-mixed",
            r#"{"rows":[{"tag":"Circle","values":[1]},{"tag":"Square","values":[]}],"index":{"p":[1,2],"q":[]},"note":null}"#,
        ),
        ("deep-nesting", r#"{"a":{"b":{"c":{"d":[1,{"e":null}]}}}}"#),
    ]
}

fn render_json_value(decoded: Result<Value, String>) -> String {
    match decoded {
        Ok(value) => topaz_value::value::render(&value),
        Err(message) => format!("Err({message})"),
    }
}

/// The render each corpus case produced before the untyped walk began consuming
/// the tree it is given.
const RECORDED_JSON_VALUES: &[(&str, &str)] = &[
    ("null", "None"),
    ("true", "true"),
    ("false", "false"),
    ("string", "plain"),
    ("string-escapes", "a\"b\\c\nd\te"),
    ("string-unicode", "é😀"),
    ("string-empty", ""),
    ("int-zero", "0"),
    ("int-negative-zero", "0"),
    ("int", "42"),
    ("int-negative", "-7"),
    ("int-max", "9223372036854775807"),
    ("int-min", "-9223372036854775808"),
    ("int-over-i64", "92233720368547760000"),
    ("int-from-exponent", "10000000000"),
    ("int-from-trailing-zero-fraction", "1"),
    ("float", "1.5"),
    ("float-exponent", "-0.00225"),
    ("array-empty", "[]"),
    ("array-ints", "[1, 2, 3]"),
    ("array-mixed", "[None, true, s, 1.5]"),
    ("array-nested", "[[1], [2, [3]], []]"),
    ("object-empty", "{  }"),
    ("object-key-order", "{ a: 1, b: 2 }"),
    ("object-nested", "{ outer: { flag: false, inner: [1, 2] } }"),
    ("object-null-member", "{ a: None, b: 1 }"),
    (
        "object-wide",
        "{ k01: 1, k02: 2, k03: 3, k04: 4, k05: 5, k06: 6, k07: 7, k08: 8, k09: 9, k10: 10, k11: 11, k12: 12, k13: 13, k14: 14 }",
    ),
    (
        "array-of-objects",
        "[{ a: 1, b: x }, { a: 2, b: y }, { a: 3, b: z }]",
    ),
    (
        "deep-mixed",
        "{ index: { p: [1, 2], q: [] }, note: None, rows: [{ tag: Circle, values: [1] }, { tag: Square, values: [] }] }",
    ),
    (
        "deep-nesting",
        "{ a: { b: { c: { d: [1, { e: None }] } } } }",
    ),
];

#[test]
fn json_value_corpus_and_recorded_values_stay_paired() {
    let corpus = json_value_corpus();
    assert_eq!(corpus.len(), RECORDED_JSON_VALUES.len());
    for ((name, _), (recorded, _)) in corpus.iter().zip(RECORDED_JSON_VALUES) {
        assert_eq!(name, recorded);
    }
}

#[test]
fn json_to_value_owned_builds_every_corpus_case_as_recorded() {
    for ((name, input), (_, expected)) in json_value_corpus().iter().zip(RECORDED_JSON_VALUES) {
        let tree = topaz_value::value::json_parse(input).expect("corpus input parses");
        assert_eq!(
            &render_json_value(json_to_value_owned(tree)),
            expected,
            "case `{name}`"
        );
    }
}

#[test]
fn json_to_value_agrees_with_the_owned_walk_and_keeps_its_argument() {
    for (name, input) in json_value_corpus() {
        // `JSON.parseAs` owns the tree it parsed and may take it apart;
        // `JSON.decode` is handed a tree the program still holds and must not.
        let reference = topaz_value::value::json_parse(input).expect("corpus input parses");
        let node = Rc::new(topaz_value::value::json_parse(input).expect("corpus input parses"));
        let borrowed = render_json_value(json_to_value(&node));
        let owned = render_json_value(json_to_value_owned(
            topaz_value::value::json_parse(input).expect("corpus input parses"),
        ));
        assert_eq!(borrowed, owned, "case `{name}`");
        assert_eq!(
            *node, reference,
            "case `{name}` lost its JSONValue argument"
        );
    }
}

#[test]
fn json_to_value_owned_keeps_every_element_across_release_batches() {
    // The consuming walk releases array elements in batches, so a document
    // longer than two batches crosses every kind of batch boundary.
    let count = 2 * crate::runtime::model::JSON_RELEASE_BATCH + 3;
    let input = format!(
        "[{}]",
        (0..count)
            .map(|index| format!(r#"{{"i":{index},"s":"v{index}","n":[{index},null]}}"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    let owned = json_to_value_owned(topaz_value::value::json_parse(&input).expect("parses"))
        .expect("decodes");
    let Value::Array(items) = &owned else {
        panic!("expected an array");
    };
    let items = items.borrow();
    assert_eq!(items.len(), count);
    for (index, item) in items.iter().enumerate() {
        let Value::Record(fields) = item else {
            panic!("element {index} is not a record");
        };
        assert!(
            matches!(fields.get("i"), Some(Value::Int(value)) if *value == index as i64),
            "element {index} field i"
        );
        assert!(
            matches!(fields.get("s"), Some(Value::Str(value)) if **value == *format!("v{index}")),
            "element {index} field s"
        );
    }
    let borrowed = json_to_value(&topaz_value::value::json_parse(&input).expect("parses"));
    assert_eq!(
        render_json_value(borrowed),
        render_json_value(Ok(owned.clone()))
    );
}

#[test]
fn static_assertion_without_print_keeps_the_supplied_host() {
    let mut call = operation("expression/call", "");
    call.call_target = "builtin::Test".to_string();
    call.call_method = "assertEq".to_string();
    assert!(operation_requires_host(&call));
}
