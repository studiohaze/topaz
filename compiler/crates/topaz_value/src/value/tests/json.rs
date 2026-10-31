use super::*;

#[test]
fn json_utf8_cursor_preserves_numbers_and_error_columns() {
    let parsed = json_parse(r#"["한글😀",1e2,-0.50e+2,"\uD83D\uDE00"]"#).unwrap();
    let mut encoded = String::new();
    write_json_node(&mut encoded, &parsed);
    assert_eq!(encoded, r#"["한글😀",1e2,-0.50e+2,"😀"]"#);
    for (input, position) in [
        ("[\"한글😀\",1e2,]", (1, 12)),
        ("[\"한글😀\",\n1e2,]", (2, 5)),
    ] {
        let error = json_parse(input).unwrap_err();
        assert_eq!((error.line, error.column), position, "{input}");
    }
}

#[test]
fn json_stringify_canonical_encodes_and_rejects() {
    let j = |v: &Value| json_stringify(v, true);
    // scalars
    assert_eq!(j(&Value::Int(42)), Ok("42".to_string()));
    assert_eq!(j(&Value::Bool(true)), Ok("true".to_string()));
    assert_eq!(j(&Value::Unit), Ok("null".to_string()));
    assert_eq!(j(&Value::None), Ok("null".to_string()));
    assert_eq!(j(&Value::Some(Rc::new(Value::Int(7)))), Ok("7".to_string()));
    // string escaping
    assert_eq!(
        j(&Value::str("a\"b\\c\n")),
        Ok("\"a\\\"b\\\\c\\n\"".to_string())
    );
    // array preserves order
    assert_eq!(
        j(&Value::array(vec![Value::Int(1), Value::Int(2)])),
        Ok("[1,2]".to_string())
    );
    // record keys are canonical (sorted) — insert out of order, expect sorted
    let rec = Value::record([
        ("b".to_string(), Value::Int(2)),
        ("a".to_string(), Value::str("x")),
    ]);
    assert_eq!(j(&rec), Ok("{\"a\":\"x\",\"b\":2}".to_string()));
    // float / Set / function are rejected with a pathful message
    assert!(
        j(&Value::Float(1.5))
            .unwrap_err()
            .contains("float is not supported")
    );
    let mut set = OrderedSet::new();
    let _ = set.add_value(&Value::Int(1));
    assert!(
        json_stringify(&Value::Set(Rc::new(RefCell::new(set))), true)
            .unwrap_err()
            .contains("not JSON-encodable")
    );
}

#[test]
fn json_decode_unit_and_null_literal_are_distinct() {
    assert_eq!(
        render(&builtin_json_parse_as(Value::str("null"), &Schema::Unit, SP).unwrap()),
        "Ok(())"
    );
    assert_eq!(
        render(&builtin_json_parse_as(Value::str("null"), &Schema::Null, SP).unwrap()),
        "Ok(null)"
    );
}

#[test]
fn json_exact_int_handles_i128_exponent_boundaries_without_overflow() {
    fn number_int(text: &str) -> Option<i64> {
        match json_parse(text).expect("valid JSON number") {
            JsonValue::Number(number) => number.int,
            other => panic!("expected number, got {other:?}"),
        }
    }

    let i128_max = "170141183460469231731687303715884105727";
    assert_eq!(number_int(&format!("1e{i128_max}")), None);
    assert_eq!(number_int(&format!("0e{i128_max}")), Some(0));
    assert_eq!(
        number_int("0e9999999999999999999999999999999999999999"),
        None
    );
}

// --- Scalar checked-arith leaf (Part A, v5.4 native-emit substrate) ---

// --- §22 typed-decode equivalence corpus ---------------------------------
//
// `JSON.parseAs` and `JSON.decode` feed the same `(JSONValue, Schema)` pair to
// the same walk, so every input must decode to the same value and every
// rejection must carry the same path and wording. RECORDED_DECODES pins those
// renders to what the corpus produced before the walk learned to consume the
// tree it was given, so a change in meaning shows up as a diff rather than as
// silent drift through everything in the product that decodes JSON.

fn schema_field(name: &str, schema: Schema, default: Option<Value>) -> SchemaField {
    (Rc::from(name), schema, default)
}

fn struct_record(fields: Vec<SchemaField>) -> Schema {
    Schema::StructRecord {
        fields: Rc::from(fields),
    }
}

fn nominal_record(id: &str, identity: Option<&str>, fields: Vec<SchemaField>) -> Schema {
    Schema::Record {
        record_id: Rc::from(id),
        declaration_identity: identity.map(Rc::from),
        fields: Rc::from(fields),
    }
}

fn shape_enum(identity: Option<&str>) -> Schema {
    Schema::Enum {
        enum_id: Rc::from("m.Shape"),
        declaration_identity: identity.map(Rc::from),
        variants: Rc::from(vec![
            ("Circle".into(), 0u32, Rc::from(vec![Schema::Int])),
            ("Square".into(), 1u32, Rc::from(Vec::new())),
            (
                "Pair".into(),
                2u32,
                Rc::from(vec![Schema::Str, Schema::Int]),
            ),
        ]),
    }
}

fn decode_corpus() -> Vec<(&'static str, Schema, &'static str)> {
    let pair = || {
        vec![
            schema_field("a", Schema::Int, None),
            schema_field("b", Schema::Str, None),
        ]
    };
    vec![
        ("int", Schema::Int, "7"),
        ("int-from-exponent", Schema::Int, "1e10"),
        ("int-non-integer", Schema::Int, "1.5"),
        ("int-wrong-kind", Schema::Int, "\"x\""),
        ("str", Schema::Str, "\"hi\\nthere\""),
        ("str-escapes", Schema::Str, "\"\\u00e9\\ud83d\\ude00\""),
        ("str-wrong-kind", Schema::Str, "12"),
        ("bool", Schema::Bool, "false"),
        ("bool-wrong-kind", Schema::Bool, "null"),
        ("unit", Schema::Unit, "null"),
        ("unit-wrong-kind", Schema::Unit, "0"),
        ("null", Schema::Null, "null"),
        ("null-wrong-kind", Schema::Null, "[]"),
        ("option-none", Schema::Option(Rc::new(Schema::Int)), "null"),
        ("option-some", Schema::Option(Rc::new(Schema::Int)), "5"),
        (
            "option-inner-error",
            Schema::Option(Rc::new(Schema::Int)),
            "\"x\"",
        ),
        ("array", Schema::Array(Rc::new(Schema::Int)), "[1,2,3]"),
        ("array-empty", Schema::Array(Rc::new(Schema::Int)), "[]"),
        (
            "array-element-error",
            Schema::Array(Rc::new(Schema::Int)),
            "[1,\"x\",3]",
        ),
        (
            "array-nested",
            Schema::Array(Rc::new(Schema::Array(Rc::new(Schema::Str)))),
            "[[\"a\"],[\"b\",\"c\"],[]]",
        ),
        (
            "array-deep-option",
            Schema::Array(Rc::new(Schema::Array(Rc::new(Schema::Option(Rc::new(
                Schema::Int,
            )))))),
            "[[1,null],[],[2]]",
        ),
        (
            "array-wrong-kind",
            Schema::Array(Rc::new(Schema::Int)),
            "{}",
        ),
        (
            "map",
            Schema::Map(Rc::new(Schema::Int)),
            "{\"b\":2,\"a\":1}",
        ),
        (
            "map-key-order",
            Schema::Map(Rc::new(Schema::Int)),
            "{\"z\":1,\"a\":2,\"m\":3}",
        ),
        ("map-empty", Schema::Map(Rc::new(Schema::Int)), "{}"),
        (
            "map-value-error",
            Schema::Map(Rc::new(Schema::Int)),
            "{\"a\":true}",
        ),
        (
            "map-of-array",
            Schema::Map(Rc::new(Schema::Array(Rc::new(Schema::Int)))),
            "{\"x\":[1,2],\"y\":[]}",
        ),
        ("map-wrong-kind", Schema::Map(Rc::new(Schema::Int)), "[1,2]"),
        (
            "struct-record",
            struct_record(pair()),
            "{\"a\":1,\"b\":\"z\"}",
        ),
        (
            "struct-record-extra-keys",
            struct_record(pair()),
            "{\"a\":1,\"b\":\"z\",\"c\":[1,2,3],\"d\":{\"e\":null}}",
        ),
        ("struct-record-missing", struct_record(pair()), "{\"a\":1}"),
        (
            "struct-record-field-error",
            struct_record(pair()),
            "{\"a\":1,\"b\":2}",
        ),
        ("struct-record-wrong-kind", struct_record(pair()), "[]"),
        (
            "struct-record-json-field",
            struct_record(vec![
                schema_field("raw", Schema::Json, None),
                schema_field("n", Schema::Int, None),
            ]),
            "{\"n\":2,\"raw\":{\"z\":[true,null]}}",
        ),
        (
            "nominal-record",
            nominal_record(
                "m.User",
                Some("m.User#1"),
                vec![
                    schema_field("name", Schema::Str, None),
                    schema_field("age", Schema::Int, Some(Value::Int(30))),
                ],
            ),
            "{\"age\":7,\"name\":\"a\"}",
        ),
        (
            "nominal-record-default",
            nominal_record(
                "m.User",
                Some("m.User#1"),
                vec![
                    schema_field("name", Schema::Str, None),
                    schema_field("age", Schema::Int, Some(Value::Int(30))),
                ],
            ),
            "{\"name\":\"a\"}",
        ),
        (
            "nominal-record-declaration-order",
            nominal_record(
                "m.Ordered",
                None,
                vec![
                    schema_field("b", Schema::Int, None),
                    schema_field("a", Schema::Int, None),
                ],
            ),
            "{\"a\":1,\"b\":2}",
        ),
        (
            "nominal-record-missing",
            nominal_record(
                "m.User",
                Some("m.User#1"),
                vec![schema_field("name", Schema::Str, None)],
            ),
            "{}",
        ),
        (
            "nominal-record-nested",
            nominal_record(
                "m.Box",
                None,
                vec![schema_field(
                    "items",
                    Schema::Array(Rc::new(struct_record(vec![
                        schema_field("a", Schema::Int, None),
                        schema_field("b", Schema::Str, None),
                    ]))),
                    None,
                )],
            ),
            "{\"items\":[{\"a\":1,\"b\":\"x\"},{\"a\":2,\"b\":\"y\"},{\"a\":3,\"b\":\"z\"}]}",
        ),
        (
            "nominal-record-wrong-kind",
            nominal_record(
                "m.User",
                None,
                vec![schema_field("name", Schema::Str, None)],
            ),
            "7",
        ),
        (
            "enum-payload",
            shape_enum(Some("m.Shape#1")),
            "{\"tag\":\"Circle\",\"values\":[3]}",
        ),
        (
            "enum-no-values-key",
            shape_enum(Some("m.Shape#1")),
            "{\"tag\":\"Square\"}",
        ),
        (
            "enum-empty-values",
            shape_enum(Some("m.Shape#1")),
            "{\"tag\":\"Square\",\"values\":[]}",
        ),
        (
            "enum-two-payloads",
            shape_enum(None),
            "{\"tag\":\"Pair\",\"values\":[\"a\",1]}",
        ),
        (
            "enum-extra-keys",
            shape_enum(None),
            "{\"extra\":[1,2,3],\"tag\":\"Circle\",\"values\":[3]}",
        ),
        (
            "enum-arity-mismatch",
            shape_enum(None),
            "{\"tag\":\"Pair\",\"values\":[\"a\"]}",
        ),
        ("enum-unknown-tag", shape_enum(None), "{\"tag\":\"Nope\"}"),
        ("enum-tag-wrong-kind", shape_enum(None), "{\"tag\":1}"),
        ("enum-missing-tag", shape_enum(None), "{\"values\":[]}"),
        (
            "enum-values-wrong-kind",
            shape_enum(None),
            "{\"tag\":\"Square\",\"values\":1}",
        ),
        (
            "enum-payload-error",
            shape_enum(None),
            "{\"tag\":\"Pair\",\"values\":[1,2]}",
        ),
        ("enum-wrong-kind", shape_enum(None), "[]"),
        (
            "newtype",
            Schema::Newtype {
                newtype_id: Rc::from("m.UserId"),
                declaration_identity: Some(Rc::from("m.UserId#1")),
                base: Rc::new(Schema::Int),
            },
            "5",
        ),
        (
            "newtype-no-identity",
            Schema::Newtype {
                newtype_id: Rc::from("m.UserId"),
                declaration_identity: None,
                base: Rc::new(Schema::Int),
            },
            "5",
        ),
        (
            "newtype-inner-error",
            Schema::Newtype {
                newtype_id: Rc::from("m.UserId"),
                declaration_identity: None,
                base: Rc::new(Schema::Int),
            },
            "\"x\"",
        ),
        (
            "json-passthrough",
            Schema::Json,
            "{\"b\":[1,{\"c\":null}],\"a\":\"s\"}",
        ),
        (
            "array-of-records",
            Schema::Array(Rc::new(nominal_record(
                "m.Row",
                Some("m.Row#1"),
                vec![
                    schema_field("k", Schema::Str, None),
                    schema_field("v", Schema::Option(Rc::new(Schema::Int)), None),
                ],
            ))),
            "[{\"k\":\"a\",\"v\":1},{\"k\":\"b\",\"v\":null},{\"k\":\"c\",\"v\":3}]",
        ),
        (
            "mixed-nesting",
            struct_record(vec![
                schema_field("rows", Schema::Array(Rc::new(shape_enum(None))), None),
                schema_field(
                    "index",
                    Schema::Map(Rc::new(Schema::Array(Rc::new(Schema::Int)))),
                    None,
                ),
                schema_field("note", Schema::Option(Rc::new(Schema::Str)), None),
            ]),
            "{\"index\":{\"p\":[1,2],\"q\":[]},\"note\":null,\"rows\":[{\"tag\":\"Circle\",\"values\":[1]},{\"tag\":\"Square\"}]}",
        ),
    ]
}

/// The render each corpus case produced before `JSON.parseAs` began consuming
/// the tree it parses. Decoding is reached from everything in the product that
/// reads JSON, so a change of meaning has to surface here rather than travel.
const RECORDED_DECODES: &[(&str, &str)] = &[
    ("int", "Ok(7)"),
    ("int-from-exponent", "Ok(10000000000)"),
    (
        "int-non-integer",
        "Err($: expected an integer, found a non-integer number)",
    ),
    ("int-wrong-kind", "Err($: expected int, found string)"),
    ("str", "Ok(hi\nthere)"),
    ("str-escapes", "Ok(é😀)"),
    ("str-wrong-kind", "Err($: expected string, found number)"),
    ("bool", "Ok(false)"),
    ("bool-wrong-kind", "Err($: expected bool, found null)"),
    ("unit", "Ok(())"),
    ("unit-wrong-kind", "Err($: expected null, found number)"),
    ("null", "Ok(null)"),
    ("null-wrong-kind", "Err($: expected null, found array)"),
    ("option-none", "Ok(None)"),
    ("option-some", "Ok(Some(5))"),
    ("option-inner-error", "Err($: expected int, found string)"),
    ("array", "Ok([1, 2, 3])"),
    ("array-empty", "Ok([])"),
    (
        "array-element-error",
        "Err($[1]: expected int, found string)",
    ),
    ("array-nested", "Ok([[a], [b, c], []])"),
    ("array-deep-option", "Ok([[Some(1), None], [], [Some(2)]])"),
    ("array-wrong-kind", "Err($: expected array, found object)"),
    ("map", "Ok(Map{a: 1, b: 2})"),
    ("map-key-order", "Ok(Map{a: 2, m: 3, z: 1})"),
    ("map-empty", "Ok(Map{})"),
    ("map-value-error", "Err($.a: expected int, found bool)"),
    ("map-of-array", "Ok(Map{x: [1, 2], y: []})"),
    ("map-wrong-kind", "Err($: expected object, found array)"),
    ("struct-record", "Ok({ a: 1, b: z })"),
    ("struct-record-extra-keys", "Ok({ a: 1, b: z })"),
    (
        "struct-record-missing",
        "Err($: missing required field `b`)",
    ),
    (
        "struct-record-field-error",
        "Err($.b: expected string, found number)",
    ),
    (
        "struct-record-wrong-kind",
        "Err($: expected object, found array)",
    ),
    (
        "struct-record-json-field",
        "Ok({ n: 2, raw: {\"z\":[true,null]} })",
    ),
    ("nominal-record", "Ok(m.User { name: a, age: 7 })"),
    ("nominal-record-default", "Ok(m.User { name: a, age: 30 })"),
    (
        "nominal-record-declaration-order",
        "Ok(m.Ordered { b: 2, a: 1 })",
    ),
    (
        "nominal-record-missing",
        "Err($: missing required field `name`)",
    ),
    (
        "nominal-record-nested",
        "Ok(m.Box { items: [{ a: 1, b: x }, { a: 2, b: y }, { a: 3, b: z }] })",
    ),
    (
        "nominal-record-wrong-kind",
        "Err($: expected object, found number)",
    ),
    ("enum-payload", "Ok(m.Shape.Circle(3))"),
    ("enum-no-values-key", "Ok(m.Shape.Square)"),
    ("enum-empty-values", "Ok(m.Shape.Square)"),
    ("enum-two-payloads", "Ok(m.Shape.Pair(a, 1))"),
    ("enum-extra-keys", "Ok(m.Shape.Circle(3))"),
    (
        "enum-arity-mismatch",
        "Err($: variant `Pair` expects 2 value(s), found 1)",
    ),
    ("enum-unknown-tag", "Err($: unknown variant tag `Nope`)"),
    (
        "enum-tag-wrong-kind",
        "Err($.tag: expected string, found number)",
    ),
    ("enum-missing-tag", "Err($: missing enum `tag` field)"),
    (
        "enum-values-wrong-kind",
        "Err($.values: expected array, found number)",
    ),
    (
        "enum-payload-error",
        "Err($.values[0]: expected string, found number)",
    ),
    (
        "enum-wrong-kind",
        "Err($: expected an enum object, found array)",
    ),
    ("newtype", "Ok(m.UserId(5))"),
    ("newtype-no-identity", "Ok(m.UserId(5))"),
    ("newtype-inner-error", "Err($: expected int, found string)"),
    (
        "json-passthrough",
        "Ok({\"a\":\"s\",\"b\":[1,{\"c\":null}]})",
    ),
    (
        "array-of-records",
        "Ok([m.Row { k: a, v: Some(1) }, m.Row { k: b, v: None }, m.Row { k: c, v: Some(3) }])",
    ),
    (
        "mixed-nesting",
        "Ok({ index: Map{p: [1, 2], q: []}, note: None, rows: [m.Shape.Circle(1), m.Shape.Square] })",
    ),
];

#[test]
fn decode_corpus_and_recorded_decodes_stay_paired() {
    let corpus = decode_corpus();
    assert_eq!(corpus.len(), RECORDED_DECODES.len());
    for ((name, _, _), (recorded, _)) in corpus.iter().zip(RECORDED_DECODES) {
        assert_eq!(name, recorded);
    }
}

#[test]
fn json_parse_as_decodes_every_corpus_case_as_recorded() {
    for ((name, schema, input), (_, expected)) in decode_corpus().iter().zip(RECORDED_DECODES) {
        let decoded = builtin_json_parse_as(Value::str(input), schema, SP).unwrap();
        assert_eq!(&render(&decoded), expected, "case `{name}`");
    }
}

#[test]
fn json_decode_agrees_with_json_parse_as_and_keeps_its_argument() {
    for (name, schema, input) in decode_corpus() {
        // `JSON.parseAs` owns the tree it parsed and may take it apart;
        // `JSON.decode` is handed a tree the program still holds and must not.
        let reference = json_parse(input).expect("corpus input parses");
        let node = Rc::new(json_parse(input).expect("corpus input parses"));
        let by_decode = builtin_json_decode(Value::Json(node.clone()), &schema, SP).unwrap();
        let by_parse_as = builtin_json_parse_as(Value::str(input), &schema, SP).unwrap();
        assert_eq!(render(&by_decode), render(&by_parse_as), "case `{name}`");
        assert_eq!(
            *node, reference,
            "case `{name}` lost its JSONValue argument"
        );
    }
}
