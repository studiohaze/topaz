use super::*;
use crate::program::decode_json::parse_program;

fn literal_payload(kind: &str, detail: &str, encoding: Option<&str>) -> String {
    let encoding = encoding.map_or(String::new(), |value| {
        format!(",\"literalEncoding\":{value:?}")
    });
    format!(
        r#"{{"schema":"topaz.compiler.fixed-point-ir-payload/v1","loweredModules":[],"loweredOperations":[{{
        "id":"literal","module":"test","lo":17,"hi":23,"kind":{kind:?},"detail":{detail:?}{encoding},
        "operands":[],"operandLabels":[],"semanticType":"","bindingMutable":false,
        "referenceIdentity":"","bindingName":"","declarationIdentity":"","controlTarget":"",
        "callTarget":"","callCalleeKind":"","callMethod":"","callOptional":false,
        "callShadowFirst":false,"callStageMethod":"","callArguments":[],"callEvaluations":[]
    }}]}}"#
    )
}

fn literal_image(kind: &str, detail: &str, encoding: Option<&str>) -> Vec<u8> {
    let mut bytes = b"TPZIMAGE\x01".to_vec();
    if encoding.is_some() {
        compact_u32(&mut bytes, topaz_value::LITERAL_TABLE_MARKER);
        compact_string(&mut bytes, topaz_value::LITERAL_TABLE_ENCODING);
    }
    compact_u32(&mut bytes, 1);
    for value in ["literal", "test", kind, detail] {
        compact_string(&mut bytes, value);
    }
    if let Some(encoding) = encoding {
        compact_string(&mut bytes, encoding);
    }
    for _ in 0..5 {
        compact_string(&mut bytes, "");
    }
    for value in [17, 23, 0, 0, 0] {
        compact_u32(&mut bytes, value);
    }
    bytes
}

#[test]
fn literal_codecs_preserve_legacy_json_and_compact_and_checked_values() {
    for (kind, detail, encoding, expected) in [
        ("expression/float", "9.0", None, "9.0"),
        ("expression/float", "0.5", None, "0.5"),
        ("expression/float", "10", None, "10.0"),
        (
            "expression/float",
            "4024000000000000",
            None,
            "4024000000000000",
        ),
        (
            "expression/float",
            "4024000000000000",
            Some(topaz_value::CHECKED_LITERAL_ENCODING),
            "10.0",
        ),
        ("expression/string-text", "\\n", None, "\n"),
        (
            "expression/string-text",
            "\\n",
            Some(topaz_value::CHECKED_LITERAL_ENCODING),
            "\\n",
        ),
        (
            "expression/string-text",
            "\n",
            Some(topaz_value::CHECKED_LITERAL_ENCODING),
            "\n",
        ),
    ] {
        let json = parse_program(
            &literal_payload(kind, detail, encoding),
            ProgramAdmission::TargetProduct,
        )
        .expect("literal JSON")
        .program;
        let compact =
            parse_embedded_program(&literal_image(kind, detail, encoding), b"TPZIMAGE", "test")
                .expect("literal compact");
        for program in [json, compact] {
            let mut machine = Machine::new(Arc::new(program));
            let value = data(
                run_local(machine.eval_value(0, EnvironmentFrame::root()))
                    .expect("literal execution"),
            )
            .unwrap();
            assert_eq!(
                topaz_value::value::render(&value),
                expected,
                "{kind} {detail:?} {encoding:?}"
            );
        }
    }
    for (detail, encoding, milliseconds) in [
        ("1s", None, 1000),
        ("1000", Some(topaz_value::CHECKED_LITERAL_ENCODING), 1000),
        ("10", Some(topaz_value::CHECKED_LITERAL_ENCODING), 10),
    ] {
        let json = parse_program(
            &literal_payload("expression/duration", detail, encoding),
            ProgramAdmission::TargetProduct,
        )
        .unwrap()
        .program;
        let compact = parse_embedded_program(
            &literal_image("expression/duration", detail, encoding),
            b"TPZIMAGE",
            "test",
        )
        .unwrap();
        for program in [json, compact] {
            assert_eq!(
                program.literal_facts[0],
                Some(Ok(topaz_value::IrLiteral::Duration(milliseconds)))
            );
        }
    }
}

#[test]
fn legacy_timeout_codecs_keep_reached_fault_span_and_unselected_laziness() {
    struct ClockHost(Cell<usize>);
    impl Host for ClockHost {
        fn print(&self, _: &str) {}
        fn open(&self, _: &str) -> Result<topaz_value::ResourceId, String> {
            Err("not used".into())
        }
        fn read(&self, _: topaz_value::ResourceId) -> Result<String, String> {
            Err("not used".into())
        }
        fn write(&self, _: topaz_value::ResourceId, _: &str) -> Result<(), String> {
            Err("not used".into())
        }
        fn close(&self, _: topaz_value::ResourceId) {}
        fn now_millis(&self) -> u64 {
            self.0.set(self.0.get() + 1);
            0
        }
        fn defer_error(&self, _: &str) {}
        fn lispex_application(
            &self,
            _: topaz_value::LispexApplicationRequest,
        ) -> topaz_value::LispexApplicationResponse {
            topaz_value::LispexApplicationResponse::OperationalFault {
                code: "target-unavailable".into(),
                detail: None,
            }
        }
    }
    for (detail, encoding, allowed) in [
        ("1s", None, true),
        ("+1s", None, false),
        ("1_0s", None, false),
        ("s", None, false),
        ("18446744073709552s", None, false),
        ("18446744073709551616ms", None, false),
        ("18446744073709551615ms", None, true),
        ("1000", Some(topaz_value::CHECKED_LITERAL_ENCODING), true),
        (
            "18446744073709551616",
            Some(topaz_value::CHECKED_LITERAL_ENCODING),
            false,
        ),
    ] {
        for (unselected, arm_fault) in [(false, false), (false, true), (true, true)] {
            let mut concurrent = operation("expression/concurrent", "");
            concurrent.operands = vec![1, 2, 3];
            concurrent.operand_labels = vec![
                "timeout:0".into(),
                "concurrent-arm[0]=a".into(),
                "else:0".into(),
            ];
            let mut timeout = operation("expression/duration", detail);
            timeout.lo = 17;
            timeout.hi = 23;
            timeout.literal_encoding = if encoding.is_some() {
                topaz_value::IrLiteralEncoding::CheckedValues
            } else {
                topaz_value::IrLiteralEncoding::LegacySource
            };
            let mut arm = operation(
                if arm_fault {
                    "expression/binary"
                } else {
                    "expression/integer"
                },
                if arm_fault { "divide" } else { "7" },
            );
            if arm_fault {
                arm.operands = vec![4, 5];
                arm.operand_labels = vec!["lhs:0".into(), "rhs:0".into()];
            }
            let mut branch = operation("expression/if", "");
            branch.operands = vec![6, 0, 3];
            branch.operand_labels = vec!["condition:0".into(), "then:0".into(), "else:0".into()];
            let mut ops = vec![
                concurrent,
                timeout,
                arm,
                operation("expression/integer", "11"),
                operation("expression/integer", "1"),
                operation("expression/integer", "0"),
                operation("expression/boolean", "false"),
                branch,
            ];
            for (i, op) in ops.iter_mut().enumerate() {
                op.id = format!("op{i}");
            }
            for program in timeout_program_codecs(&ops) {
                let host = Rc::new(ClockHost(Cell::new(0)));
                let mut machine = Machine::new_with_facts_host_and_input(
                    Arc::new(program),
                    None,
                    "",
                    Some(host.clone()),
                )
                .unwrap();
                let result = run_local(
                    machine.eval_value(if unselected { 7 } else { 0 }, EnvironmentFrame::root()),
                );
                if unselected {
                    assert!(matches!(
                        data(result.expect("unselected timeout")).unwrap(),
                        Value::Int(11)
                    ));
                    assert_eq!(host.0.get(), 0);
                } else if !allowed {
                    let error = result.err().expect("legacy duration refusal");
                    let fault =
                        decode_runtime_diagnostic(&error).expect("original runtime diagnostic");
                    assert_eq!(fault.code, topaz_value::codes::GUARD_TYPE);
                    assert_eq!(fault.span, Span::new(FileId(0), 17, 23));
                    assert_eq!(
                        fault.message,
                        "`concurrent` timeout duration must fit in u64 milliseconds (§15)"
                    );
                    assert_eq!(host.0.get(), 0, "timeout fault must precede clock and arm");
                } else if arm_fault {
                    assert!(
                        result
                            .err()
                            .expect("arm division fault")
                            .contains("division by zero")
                    );
                    assert!(host.0.get() > 0);
                } else {
                    let value = data(result.expect("valid timeout")).unwrap();
                    assert!(
                        matches!(value,Value::Record(fields) if fields.len()==1 && matches!(fields.get("a"),Some(Value::Int(7))))
                    );
                    assert!(host.0.get() > 0);
                }
            }
        }
    }
}

/// Ordinary input graphs for the existing JSON and compact decoders.
fn timeout_program_codecs(ops: &[Operation]) -> [Program; 2] {
    let mut rows = Vec::new();
    let explicit = ops
        .iter()
        .any(|op| op.literal_encoding == topaz_value::IrLiteralEncoding::CheckedValues);
    let mut bytes = b"TPZIMAGE\x01".to_vec();
    if explicit {
        compact_u32(&mut bytes, topaz_value::LITERAL_TABLE_MARKER);
        compact_string(&mut bytes, topaz_value::LITERAL_TABLE_ENCODING);
    }
    compact_u32(&mut bytes, ops.len() as u32);
    for op in ops {
        let encoding = if op.literal_encoding == topaz_value::IrLiteralEncoding::CheckedValues {
            format!(
                ",\"literalEncoding\":{:?}",
                topaz_value::CHECKED_LITERAL_ENCODING
            )
        } else {
            String::new()
        };
        let operands: Vec<_> = op.operands.iter().map(|i| ops[*i].id.as_str()).collect();
        rows.push(format!(r#"{{"id":{:?},"module":"test","lo":{},"hi":{},"kind":{:?},"detail":{:?}{encoding},"operands":{:?},"operandLabels":{:?},"semanticType":"","bindingMutable":false,"referenceIdentity":"","bindingName":"","declarationIdentity":"","controlTarget":"","callTarget":"","callCalleeKind":"","callMethod":"","callOptional":false,"callShadowFirst":false,"callStageMethod":"","callArguments":[],"callEvaluations":[]}}"#,op.id,op.lo,op.hi,op.kind,op.detail,operands,op.operand_labels));
        for value in [&op.id, "test", &op.kind, &op.detail] {
            compact_string(&mut bytes, value);
        }
        if explicit {
            compact_string(&mut bytes, op.literal_encoding.name());
        }
        for _ in 0..5 {
            compact_string(&mut bytes, "");
        }
        compact_u32(&mut bytes, op.lo);
        compact_u32(&mut bytes, op.hi);
        compact_u32(&mut bytes, op.operands.len() as u32);
        for i in &op.operands {
            compact_u32(&mut bytes, *i as u32);
        }
        compact_u32(&mut bytes, op.operand_labels.len() as u32);
        for label in &op.operand_labels {
            compact_string(&mut bytes, label);
        }
    }
    compact_u32(&mut bytes, 0);
    let payload = format!(
        r#"{{"schema":"topaz.compiler.fixed-point-ir-payload/v1","loweredModules":[],"loweredOperations":[{}]}}"#,
        rows.join(",")
    );
    [
        parse_program(&payload, ProgramAdmission::TargetProduct)
            .expect("timeout JSON")
            .program,
        parse_embedded_program(&bytes, b"TPZIMAGE", "test").expect("timeout compact"),
    ]
}

#[test]
fn literal_codec_refuses_unknown_identity_and_malformed_checked_bits() {
    let payload = literal_payload(
        "expression/float",
        "10",
        Some(topaz_value::CHECKED_LITERAL_ENCODING),
    );
    let program = parse_program(&payload, ProgramAdmission::TargetProduct)
        .unwrap()
        .program;
    let mut machine = Machine::new(Arc::new(program));
    assert!(
        run_local(machine.eval_value(0, EnvironmentFrame::root()))
            .err()
            .expect("invalid checked bits")
            .contains("invalid checked float bits")
    );
    for encoding in ["unknown-codec", "checked-values/v2"] {
        assert!(
            parse_program(
                &literal_payload("expression/float", "10", Some(encoding)),
                ProgramAdmission::TargetProduct
            )
            .is_err()
        );
        assert!(
            parse_embedded_program(
                &literal_image("expression/float", "10", Some(encoding)),
                b"TPZIMAGE",
                "test"
            )
            .is_err()
        );
    }
    let wrong_type = payload.replace(
        "\"literalEncoding\":\"checked-values/v1\"",
        "\"literalEncoding\":false",
    );
    assert!(parse_program(&wrong_type, ProgramAdmission::TargetProduct).is_err());
}

#[test]
fn exponentiation_executes_integer_and_float_operands() {
    for (kind, base, exponent, expected) in [
        ("expression/integer", "2", "10", "1024"),
        ("expression/float", "9.0", "0.5", "3.0"),
    ] {
        let mut power = operation("expression/binary", "power");
        power.operands = vec![1, 2];
        let mut machine = Machine::new(Arc::new(Program::new(
            Vec::new(),
            vec![power, operation(kind, base), operation(kind, exponent)],
        )));
        let value =
            data(run_local(machine.eval_value(0, EnvironmentFrame::root())).expect("power"))
                .expect("power value");
        assert_eq!(topaz_value::value::render(&value), expected);
    }
}
#[test]
fn exponentiation_preserves_negative_exponent_and_overflow_diagnostics() {
    for (exponent, code) in [("-1", "TPZ4005"), ("63", "TPZ4004")] {
        let mut power = operation("expression/binary", "power");
        power.operands = vec![1, 2];
        let mut machine = Machine::new(Arc::new(Program::new(
            Vec::new(),
            vec![
                power,
                operation("expression/integer", "2"),
                operation("expression/integer", exponent),
            ],
        )));
        let error = match machine.eval(0, EnvironmentFrame::root()) {
            Err(error) => error,
            Ok(_) => panic!("invalid integer power succeeded"),
        };
        let fault = decode_runtime_diagnostic(&error).expect("structured power fault");
        assert_eq!(fault.code, code);
    }
}
