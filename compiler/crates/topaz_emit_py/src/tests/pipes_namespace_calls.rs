use super::*;

#[test]
fn emits_namespace_origin_dynamic_index_named_default_function_value_calls() {
    let generated = emit_source_with_files(
        r#"
	import util
	function main() -> int {
	    let i = 0
	    let j = 1
	    util.callbacks[i](a: 5) * 10 + (util.callbacks)[j](a: 15)
	}
	main()
	"#,
        &[(
            "util.tpz",
            r#"
	function add(a: int, b: int = 2) -> int {
	    a + b
	}

	function sub(a: int, b: int = 10) -> int {
	    a - b
	}

	export let callbacks = [add, sub]
	"#,
        )],
    );
    assert!(
        generated.matches("tpz_call(tpz_index(").count() >= 2,
        "dynamic namespace-origin calls should use runtime index calls: {generated}"
    );
    assert!(
        generated.contains("\"_t_61\": 5") && generated.contains("\"_t_61\": 15"),
        "dynamic namespace-origin calls should preserve named argument metadata without baking defaults: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        75,
        "namespace-origin dynamic-index named/default function-value direct-call parity",
    );
}

#[test]
fn emits_namespace_origin_dynamic_index_named_default_pipe_function_value_calls() {
    let generated = emit_source_with_files(
        r#"
	import util
	function main() -> int {
	    let i = 0
	    let j = 1
	    let a = 5 |> util.callbacks[i](a: _)
	    let b = 15 |> (util.callbacks)[j](a: _)
	    a * 10 + b
	}
	main()
	"#,
        &[(
            "util.tpz",
            r#"
	function add(a: int, b: int = 2) -> int {
	    a + b
	}

	function sub(a: int, b: int = 10) -> int {
	    a - b
	}

	export let callbacks = [add, sub]
	"#,
        )],
    );
    assert!(
        generated.matches("tpz_call(tpz_index(").count() >= 2
            && generated.contains("\"_t_61\": __tpz_piped"),
        "dynamic namespace-origin pipe calls should preserve named/default pipe metadata: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        75,
        "namespace-origin dynamic-index named/default function-value pipe parity",
    );
}

#[test]
fn keeps_unproven_namespace_imported_value_metadata_plain() {
    let out_of_range = emit_error_for_source_with_files(
        r#"
	import util
function main() -> int {
    util.callbacks[2](a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
export let callbacks = [add]
"#,
        )],
    );
    assert_eq!(out_of_range.code(), "TPZ6PY0001");
    match out_of_range.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "call argument shape", "out-of-range namespace index")
        }
        other => {
            panic!("out-of-range namespace index: expected unsupported error, got {other:?}")
        }
    }

    let heterogeneous_signature_dynamic_index = emit_error_for_source_with_files(
        r#"
	import util
	function main() -> int {
	    let i = 0
	    util.callbacks[i](a: 5)
	}
	main()
	"#,
        &[(
            "util.tpz",
            r#"
	function add(a: int, b: int = 2) -> int {
	    a + b
	}

	function mul(a: int, b: int) -> int {
	    a * b
	}

	export let callbacks = [add, mul]
	"#,
        )],
    );
    assert_eq!(heterogeneous_signature_dynamic_index.code(), "TPZ6PY0001");
    match heterogeneous_signature_dynamic_index.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call argument shape",
                "heterogeneous namespace-origin dynamic index"
            )
        }
        other => {
            panic!(
                "heterogeneous namespace-origin dynamic index: expected unsupported error, got {other:?}"
            )
        }
    }

    let non_callable_element_dynamic_index = emit_error_for_source_with_files(
        r#"
	import util
	function main() -> int {
	    let i = 1
	    util.callbacks[i](a: 5)
	}
	main()
	"#,
        &[(
            "util.tpz",
            r#"
	function add(a: int, b: int = 2) -> int {
	    a + b
	}

	export let callbacks = [42, add]
	"#,
        )],
    );
    assert_eq!(non_callable_element_dynamic_index.code(), "TPZ6PY0001");
    match non_callable_element_dynamic_index.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call argument shape",
                "non-callable element namespace-origin dynamic index"
            )
        }
        other => {
            panic!(
                "non-callable element namespace-origin dynamic index: expected unsupported error, got {other:?}"
            )
        }
    }

    let nested_member_dynamic_index = emit_error_for_source_with_files(
        r#"
	import util
	function main() -> int {
	    let i = 0
	    util.holder.callbacks[i](a: 5)
	}
	main()
	"#,
        &[(
            "util.tpz",
            r#"
	function add(a: int, b: int = 2) -> int {
	    a + b
	}
	export let holder = { callbacks: [add] }
	"#,
        )],
    );
    assert_eq!(nested_member_dynamic_index.code(), "TPZ6PY0001");
    match nested_member_dynamic_index.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call argument shape",
                "nested namespace-origin dynamic index"
            )
        }
        other => {
            panic!(
                "nested namespace-origin dynamic index: expected unsupported error, got {other:?}"
            )
        }
    }

    let (mutable_export_dynamic_index, mutable_export_unit) =
        emit_unchecked_error_and_unit_for_source_with_files(
            r#"
	import util
	function main() -> int {
	    let i = 0
	    util.callbacks[i](a: 5)
	}
	main()
	"#,
            &[(
                "util.tpz",
                r#"
	function add(a: int, b: int = 2) -> int {
	    a + b
	}
	export let mut callbacks = [add]
	"#,
            )],
        );
    assert!(
        !mutable_export_unit.diagnostics.is_empty(),
        "export let mut must remain a shared resolver diagnostic before normal emission"
    );
    assert_eq!(mutable_export_dynamic_index.code(), "TPZ6PY0001");
    match mutable_export_dynamic_index.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call argument shape",
                "mutable exported namespace-origin dynamic index"
            )
        }
        other => {
            panic!(
                "mutable exported namespace-origin dynamic index: expected unsupported error, got {other:?}"
            )
        }
    }

    let heterogeneous_signature_pipe = emit_error_for_source_with_files(
        r#"
	import util
	function main() -> int {
	    let i = 0
	    5 |> util.callbacks[i](a: _)
	}
	main()
	"#,
        &[(
            "util.tpz",
            r#"
	function add(a: int, b: int = 2) -> int {
	    a + b
	}

	function mul(a: int, b: int) -> int {
	    a * b
	}

	export let callbacks = [add, mul]
	"#,
        )],
    );
    assert_eq!(heterogeneous_signature_pipe.code(), "TPZ6PY0001");
    match heterogeneous_signature_pipe.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "pipe stage call target",
                "heterogeneous namespace-origin dynamic-index pipe"
            )
        }
        other => {
            panic!(
                "heterogeneous namespace-origin dynamic-index pipe: expected unsupported error, got {other:?}"
            )
        }
    }

    let shadowed_namespace = emit_error_for_source_with_files(
        r#"
	import util
function add(a: int, b: int = 2) -> int {
    a + b
}
function main() -> int {
    let util = { callbacks: [add] }
    util.callbacks[0](a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
export let callbacks = [add]
"#,
        )],
    );
    assert_eq!(shadowed_namespace.code(), "TPZ6PY0001");
    match shadowed_namespace.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "call argument shape", "shadowed namespace")
        }
        other => panic!("shadowed namespace: expected unsupported error, got {other:?}"),
    }
}

#[test]
fn emits_namespace_member_alias_dynamic_index_named_default_function_value_calls() {
    let generated = emit_source_with_files(
        r#"
import util
function main() -> int {
    let callbacks = util.callbacks
    let i = 0
    let j = 1
    callbacks[i](a: 5) * 10 + callbacks[j](a: 15)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}

function sub(a: int, b: int = 10) -> int {
    a - b
}

export let callbacks = [add, sub]
"#,
        )],
    );
    assert!(
        generated.contains("tpz_call(tpz_index("),
        "dynamic namespace-member alias calls should use runtime index calls: {generated}"
    );
    assert!(
        generated.contains("\"_t_61\": 5") && generated.contains("\"_t_61\": 15"),
        "dynamic namespace-member alias calls should preserve named argument metadata: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        75,
        "namespace member alias dynamic-index named/default function-value call parity",
    );
}
