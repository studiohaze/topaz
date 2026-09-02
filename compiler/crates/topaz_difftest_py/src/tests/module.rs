use super::*;

#[test]
fn module_import_python_corpus_matches_interpreter_and_boxed_rust() {
    assert_eq!(
        MODULE_CORE_FIXTURES.len(),
        PYTHON_MODULE_CORE_FIXTURE_COUNT,
        "Python module core corpus count drifted"
    );

    let python = cpython_31314();
    let tmp = temp_dir("topaz-difftest-py-module-core");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");

    let mut failures = Vec::new();
    for fixture in MODULE_CORE_FIXTURES {
        let interp = match run_module_interpreter(fixture) {
            Ok(receipt) => receipt,
            Err(error) => {
                failures.push(format!(
                    "{}: interpreter setup failed: {error}",
                    fixture.name
                ));
                continue;
            }
        };
        let rust = run_module_boxed_rust(fixture);
        compare_wide_receipts(fixture.name, &interp, &rust, &mut failures);

        match run_module_python(&python, &tmp, fixture) {
            Ok(trace) => {
                compare_python_trace_to_receipt(fixture.name, true, &trace, &interp, &mut failures);
            }
            Err(error) => failures.push(format!("{}: Python failed: {error}", fixture.name)),
        }
    }

    let _ = fs::remove_dir_all(&tmp);
    assert!(
        failures.is_empty(),
        "module Python corpus mismatches ({} of {}):\n{}",
        failures.len(),
        MODULE_CORE_FIXTURES.len(),
        failures.join("\n")
    );
}

#[test]
fn af002_dynamic_nominal_record_defaults_match_all_three_engines() {
    let python = cpython_31314();
    let tmp = temp_dir("topaz-difftest-py-af002");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");

    let mut failures = Vec::new();
    for name in [
        "nominal_record_mutable_default_current_binding",
        "nominal_record_effectful_defaults_order_and_skip",
    ] {
        let fixture = WIDE_CORE_FIXTURES
            .iter()
            .find(|fixture| fixture.name == name)
            .unwrap_or_else(|| panic!("missing wide fixture {name}"));
        let interp = run_wide_interpreter(fixture).expect("interpreter result");
        let rust = run_wide_boxed_rust(fixture);
        compare_wide_receipts(name, &interp, &rust, &mut failures);
        match run_wide_python(&python, &tmp, fixture) {
            Ok(trace) => {
                compare_python_trace_to_wide_receipt(fixture, &trace, &interp, &mut failures)
            }
            Err(error) => failures.push(format!("{name}: Python failed: {error}")),
        }
        if name == "nominal_record_effectful_defaults_order_and_skip" {
            assert_eq!(interp.stdout, ["2", "1", "2"]);
        }
    }

    let fixture = MODULE_CORE_FIXTURES
        .iter()
        .find(|fixture| {
            fixture.name == "module_selected_nominal_record_mutable_default_current_binding"
        })
        .expect("missing module fixture");
    let interp = run_module_interpreter(fixture).expect("module interpreter result");
    let rust = run_module_boxed_rust(fixture);
    compare_wide_receipts(fixture.name, &interp, &rust, &mut failures);
    match run_module_python(&python, &tmp, fixture) {
        Ok(trace) => {
            compare_python_trace_to_receipt(fixture.name, true, &trace, &interp, &mut failures)
        }
        Err(error) => failures.push(format!("{}: Python failed: {error}", fixture.name)),
    }

    let _ = fs::remove_dir_all(&tmp);
    assert!(
        failures.is_empty(),
        "wide fixture mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn server_contract_demo_matches_interpreter_boxed_rust_and_handwritten_python() {
    let fixture = &SERVER_CONTRACT_DEMO_FIXTURE;
    let python = cpython_31314();
    let tmp = temp_dir("topaz-difftest-py-server-contract");
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::write(tmp.join("topaz_py_rt.py"), PY_RT).expect("write runtime");

    let mut provider = InMemoryProvider::new();
    for (path, source) in fixture.files {
        provider.add_file(*path, *source);
    }
    let unit = resolve_with_version(&provider, fixture.entry, None, LangVersion::CURRENT);
    assert!(
        unit.diagnostics.is_empty(),
        "server contract fixture must resolve cleanly: {:?}",
        unit.diagnostics
    );

    let script = tmp.join("pym23_server_contract_demo.py");
    fs::write(
        &script,
        emit_module(&unit).expect("server contract fixture emits to Python"),
    )
    .expect("write generated Python");

    let cases = [
        (
            "health",
            "{\"method\":\"GET\",\"path\":\"/health\"}",
            vec!["{\"body\":\"ok\",\"status\":200}".to_string()],
        ),
        (
            "echo",
            "{\"method\":\"POST\",\"path\":\"/echo\"}",
            vec!["{\"body\":\"POST /echo\",\"status\":200}".to_string()],
        ),
        (
            "not_found",
            "{\"method\":\"GET\",\"path\":\"/missing\"}",
            vec!["{\"body\":\"/missing\",\"status\":404}".to_string()],
        ),
        ("missing_path", "{\"method\":\"GET\"}", Vec::new()),
    ];

    let mut failures = Vec::new();
    for (name, payload, expected_stdout) in cases {
        let mut seed_files = BTreeMap::new();
        seed_files.insert("request.json".to_string(), payload.to_string());

        let interp = match run_module_interpreter_with_files(fixture, &seed_files) {
            Ok(receipt) => receipt,
            Err(error) => {
                failures.push(format!("{name}: interpreter setup failed: {error}"));
                continue;
            }
        };
        let rust = run_module_boxed_rust_with_files(fixture, &seed_files);
        compare_wide_receipts(name, &interp, &rust, &mut failures);

        match run_python_once_with_files(&python, &tmp, &script, "", &seed_files) {
            Ok(trace) => {
                compare_python_trace_to_receipt(name, true, &trace, &interp, &mut failures);
                if trace.stdout != expected_stdout {
                    failures.push(format!(
                        "{name}: contract stdout mismatch\n  python: {:?}\n  expected: {:?}",
                        trace.stdout, expected_stdout
                    ));
                }
                if let Some(fault) = &trace.fault
                    && (fault.span.file < 0 || fault.span.hi < fault.span.lo)
                {
                    failures.push(format!("{name}: malformed trace v1 fault span: {fault:?}"));
                }
            }
            Err(error) => failures.push(format!("{name}: Python failed: {error}")),
        }
    }

    let report = run_server_contract_python_report(&python, &tmp, &script)
        .unwrap_or_else(|error| panic!("server contract report failed: {error}"));
    if !report.contains("\"failed\":0") {
        failures.push(format!("contract report contains failures:\n{report}"));
    }
    if !report.contains("\"raw_python_exceptions\":0") {
        failures.push(format!(
            "contract report contains raw Python exceptions:\n{report}"
        ));
    }
    if !report.contains("\"demo\":\"pym23_server_contract_demo\"") {
        failures.push(format!("contract report omitted demo id:\n{report}"));
    }

    let _ = fs::remove_dir_all(&tmp);
    assert!(
        failures.is_empty(),
        "server contract Python demo mismatches ({}):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
