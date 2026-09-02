use super::*;

#[test]
fn keeps_unproven_namespace_member_alias_value_metadata_plain() {
    let out_of_range = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let callbacks = util.callbacks
    callbacks[2](a: 5)
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
            assert_eq!(
                what, "call argument shape",
                "out-of-range namespace member alias index"
            )
        }
        other => {
            panic!(
                "out-of-range namespace member alias index: expected unsupported error, got {other:?}"
            )
        }
    }

    let heterogeneous_signature_dynamic_index = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let callbacks = util.callbacks
    let i = 0
    callbacks[i](a: 5)
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
                "heterogeneous namespace member alias dynamic index"
            )
        }
        other => {
            panic!(
                "heterogeneous namespace member alias dynamic index: expected unsupported error, got {other:?}"
            )
        }
    }

    let non_callable_element_dynamic_index = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let callbacks = util.callbacks
    let i = 1
    callbacks[i](a: 5)
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
                "non-callable element namespace member alias dynamic index"
            )
        }
        other => {
            panic!(
                "non-callable element namespace member alias dynamic index: expected unsupported error, got {other:?}"
            )
        }
    }

    let mutable_dynamic_index = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let mut callbacks = util.callbacks
    let i = 0
    callbacks[i](a: 5)
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
    assert_eq!(mutable_dynamic_index.code(), "TPZ6PY0001");
    match mutable_dynamic_index.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call argument shape",
                "mutable namespace member alias dynamic index"
            )
        }
        other => {
            panic!(
                "mutable namespace member alias dynamic index: expected unsupported error, got {other:?}"
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
    let callbacks = util.callbacks
    callbacks[0](a: 5)
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
            assert_eq!(
                what, "call argument shape",
                "shadowed namespace member alias"
            )
        }
        other => {
            panic!("shadowed namespace member alias: expected unsupported error, got {other:?}")
        }
    }

    let mutable_reassigned_direct = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let mut callbacks = util.callbacks
    callbacks = [util.add]
    callbacks[0](a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}
export let add = addImpl
export let callbacks = [addImpl]
"#,
        )],
    );
    assert_eq!(mutable_reassigned_direct.code(), "TPZ6PY0001");
    match mutable_reassigned_direct.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call argument shape",
                "reassigned mutable namespace member alias"
            )
        }
        other => {
            panic!(
                "reassigned mutable namespace member alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let mismatched_conditional_callable_alias = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let f = if true {
        util.add
    } else {
        util.mul
    }
    f(a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function mulImpl(x: int, y: int) -> int {
    x * y
}

export let add = addImpl
export let mul = mulImpl
"#,
        )],
    );
    assert_eq!(mismatched_conditional_callable_alias.code(), "TPZ6PY0001");
    match mismatched_conditional_callable_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call target",
                "mismatched conditional namespace member callable alias"
            )
        }
        other => {
            panic!(
                "mismatched conditional namespace member callable alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let default_presence_mismatched_conditional_callable_alias = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let f = if true {
        util.add
    } else {
        util.sub
    }
    f(a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function subImpl(a: int, b: int) -> int {
    a - b
}

export let add = addImpl
export let sub = subImpl
"#,
        )],
    );
    assert_eq!(
        default_presence_mismatched_conditional_callable_alias.code(),
        "TPZ6PY0001"
    );
    match default_presence_mismatched_conditional_callable_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call target",
                "default-presence mismatched conditional namespace member callable alias"
            )
        }
        other => {
            panic!(
                "default-presence mismatched conditional namespace member callable alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let default_presence_mismatched_conditional_pipe_alias = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let f = if true {
        util.add
    } else {
        util.sub
    }
    5 |> f(a: _)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function subImpl(a: int, b: int) -> int {
    a - b
}

export let add = addImpl
export let sub = subImpl
"#,
        )],
    );
    assert_eq!(
        default_presence_mismatched_conditional_pipe_alias.code(),
        "TPZ6PY0001"
    );
    match default_presence_mismatched_conditional_pipe_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "pipe stage call target",
                "default-presence mismatched conditional namespace member pipe alias"
            )
        }
        other => {
            panic!(
                "default-presence mismatched conditional namespace member pipe alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let variadic_tail_mismatched_conditional_pipe_alias = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let f = if true {
        util.sum
    } else {
        util.pack
    }
    5 |> f(_, ...[1, 2])
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function sumImpl(seed: int, base: int = 2, ...xs: int) -> int {
    seed + base
}

function packImpl(seed: int, base: int = 100, ...ys: int) -> int {
    base - seed
}

export let sum = sumImpl
export let pack = packImpl
"#,
        )],
    );
    assert_eq!(
        variadic_tail_mismatched_conditional_pipe_alias.code(),
        "TPZ6PY0001"
    );
    match variadic_tail_mismatched_conditional_pipe_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "pipe stage call target",
                "variadic-tail mismatched conditional namespace member pipe alias"
            )
        }
        other => {
            panic!(
                "variadic-tail mismatched conditional namespace member pipe alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let variadic_tail_mismatched_conditional_named_tail_pipe_alias =
        emit_error_for_source_with_files(
            r#"
import util
function main() -> int {
    let f = if true {
        util.sum
    } else {
        util.pack
    }
    5 |> f(...[1, 2], seed: 9, base: _)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function sumImpl(seed: int = 0, base: int = 2, ...xs: int) -> int {
    seed + base
}

function packImpl(seed: int = 0, base: int = 100, ...ys: int) -> int {
    base - seed
}

export let sum = sumImpl
export let pack = packImpl
"#,
            )],
        );
    assert_eq!(
        variadic_tail_mismatched_conditional_named_tail_pipe_alias.code(),
        "TPZ6PY0001"
    );
    match variadic_tail_mismatched_conditional_named_tail_pipe_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "pipe stage call target",
                "named-tail variadic mismatched conditional namespace member pipe alias"
            )
        }
        other => {
            panic!(
                "named-tail variadic mismatched conditional namespace member pipe alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let callable_util_files = [(
        "util.tpz",
        r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}
export let add = addImpl
"#,
    )];
    let statementful_conditional_callable_alias = emit_source_with_files(
        r#"
import util
function main() -> int {
    let f = if true {
        let marker = 1
        util.add
    } else {
        util.add
    }
    f(a: 5)
}
main()
"#,
        &callable_util_files,
    );
    assert!(
        statementful_conditional_callable_alias.contains("_t_66(_t_61=5)"),
        "inert statementful conditional namespace-member callable aliases should preserve named direct-call shape: {statementful_conditional_callable_alias}"
    );
    assert_generated_python_ok_int(
        &statementful_conditional_callable_alias,
        7,
        "inert statementful conditional namespace-member callable alias direct-call parity",
    );

    let statementful_conditional_pipe_alias = emit_source_with_files(
        r#"
import util
function main() -> int {
    let f = if true {
        let marker = 1
        util.add
    } else {
        util.add
    }
    5 |> f(a: _)
}
main()
"#,
        &callable_util_files,
    );
    assert!(
        statementful_conditional_pipe_alias.contains("lambda __tpz_piped")
            && statementful_conditional_pipe_alias.contains("_t_66(_t_61=__tpz_piped)"),
        "inert statementful conditional namespace-member callable aliases should preserve named pipe-call shape: {statementful_conditional_pipe_alias}"
    );
    assert_generated_python_ok_int(
        &statementful_conditional_pipe_alias,
        7,
        "inert statementful conditional namespace-member callable alias pipe parity",
    );

    for (name, src) in [
        (
            "mutable prefix",
            r#"
import util
function main() -> int {
    let f = if true {
        let mut marker = 1
        util.add
    } else {
        util.add
    }
    f(a: 5)
}
main()
"#,
        ),
        (
            "duplicate prefix binding",
            r#"
import util
function main() -> int {
    let f = if true {
        let marker = 1
        let marker = 2
        util.add
    } else {
        util.add
    }
    f(a: 5)
}
main()
"#,
        ),
        (
            "nonliteral prefix initializer",
            r#"
import util
function side() -> int {
    1
}
function main() -> int {
    let f = if true {
        let marker = side()
        util.add
    } else {
        util.add
    }
    f(a: 5)
}
main()
"#,
        ),
        (
            "local alias tail",
            r#"
import util
function main() -> int {
    let f = if true {
        let g = util.add
        g
    } else {
        util.add
    }
    f(a: 5)
}
main()
"#,
        ),
        (
            "expression statement prefix",
            r#"
import util
function side() -> int {
    1
}
function main() -> int {
    let f = if true {
        side()
        util.add
    } else {
        util.add
    }
    f(a: 5)
}
main()
"#,
        ),
        (
            "assignment statement prefix",
            r#"
import util
function main() -> int {
    let f = if true {
        let mut marker = 1
        marker = 2
        util.add
    } else {
        util.add
    }
    f(a: 5)
}
main()
"#,
        ),
        (
            "while statement prefix",
            r#"
import util
function main() -> int {
    let f = if true {
        let mut marker = 1
        while marker < 2 {
            marker = marker + 1
        }
        util.add
    } else {
        util.add
    }
    f(a: 5)
}
main()
"#,
        ),
        (
            "record-field tail",
            r#"
import util
function main() -> int {
    let f = if true {
        let marker = 1
        ({ primary: util.add }).primary
    } else {
        util.add
    }
    f(a: 5)
}
main()
"#,
        ),
        (
            "nested conditional tail",
            r#"
import util
function main() -> int {
    let f = if true {
        let marker = 1
        if true {
            util.add
        } else {
            util.add
        }
    } else {
        util.add
    }
    f(a: 5)
}
main()
"#,
        ),
        (
            "match tail",
            r#"
import util
function main() -> int {
    let f = if true {
        let marker = 1
        match true {
            case true => util.add
            case _ => util.add
        }
    } else {
        util.add
    }
    f(a: 5)
}
main()
"#,
        ),
    ] {
        let error = emit_error_for_source_with_files(src, &callable_util_files);
        assert_eq!(error.code(), "TPZ6PY0001", "{name}");
        match error.kind {
            PyEmitErrorKind::Unsupported(what) => {
                assert_eq!(what, "call target", "{name}")
            }
            other => {
                panic!("{name}: expected unsupported call target, got {other:?}")
            }
        }
    }

    // These non-catch-all arms keep the structural arrow type equal, so the
    // checker accepts them and only the emitter shape join should decline.
    let non_catch_all_param_name_mismatched_match_callable_alias = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 2]
    let f = match seeds[0] {
        case n if n == 1 => util.add
        case n if n == 2 => util.alt
    }
    f(a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function altImpl(x: int, b: int = 2) -> int {
    x + b
}

export let add = addImpl
export let alt = altImpl
"#,
        )],
    );
    assert_eq!(
        non_catch_all_param_name_mismatched_match_callable_alias.code(),
        "TPZ6PY0001"
    );
    match non_catch_all_param_name_mismatched_match_callable_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call target",
                "non-catch-all param-name mismatched match namespace member callable alias"
            )
        }
        other => {
            panic!(
                "non-catch-all param-name mismatched match namespace member callable alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let non_catch_all_default_presence_mismatched_match_callable_alias =
        emit_error_for_source_with_files(
            r#"
import util
function main() -> int {
    let seeds = [1, 2]
    let f = match seeds[0] {
        case n if n == 1 => util.add
        case n if n == 2 => util.sub
    }
    f(a: 5)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function subImpl(a: int, b: int) -> int {
    a - b
}

export let add = addImpl
export let sub = subImpl
"#,
            )],
        );
    assert_eq!(
        non_catch_all_default_presence_mismatched_match_callable_alias.code(),
        "TPZ6PY0001"
    );
    match non_catch_all_default_presence_mismatched_match_callable_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call target",
                "non-catch-all default-presence mismatched match namespace member callable alias"
            )
        }
        other => {
            panic!(
                "non-catch-all default-presence mismatched match namespace member callable alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let non_catch_all_param_name_mismatched_match_pipe_alias = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 2]
    let f = match seeds[0] {
        case n if n == 1 => util.add
        case n if n == 2 => util.alt
    }
    5 |> f(a: _)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function altImpl(x: int, b: int = 2) -> int {
    x + b
}

export let add = addImpl
export let alt = altImpl
"#,
        )],
    );
    assert_eq!(
        non_catch_all_param_name_mismatched_match_pipe_alias.code(),
        "TPZ6PY0001"
    );
    match non_catch_all_param_name_mismatched_match_pipe_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "pipe stage call target",
                "non-catch-all param-name mismatched match namespace member pipe alias"
            )
        }
        other => {
            panic!(
                "non-catch-all param-name mismatched match namespace member pipe alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let non_catch_all_default_presence_mismatched_match_pipe_alias =
        emit_error_for_source_with_files(
            r#"
import util
function main() -> int {
    let seeds = [1, 2]
    let f = match seeds[0] {
        case n if n == 1 => util.add
        case n if n == 2 => util.sub
    }
    5 |> f(a: _)
}
main()
"#,
            &[(
                "util.tpz",
                r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function subImpl(a: int, b: int) -> int {
    a - b
}

export let add = addImpl
export let sub = subImpl
"#,
            )],
        );
    assert_eq!(
        non_catch_all_default_presence_mismatched_match_pipe_alias.code(),
        "TPZ6PY0001"
    );
    match non_catch_all_default_presence_mismatched_match_pipe_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "pipe stage call target",
                "non-catch-all default-presence mismatched match namespace member pipe alias"
            )
        }
        other => {
            panic!(
                "non-catch-all default-presence mismatched match namespace member pipe alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let non_catch_all_variadic_tail_mismatched_match_pipe_alias = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 2]
    let f = match seeds[0] {
        case n if n == 1 => util.sum
        case n if n == 2 => util.pack
    }
    5 |> f(_, ...[1, 2])
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function sumImpl(seed: int, base: int = 2, ...xs: int) -> int {
    seed + base
}

function packImpl(seed: int, base: int = 100, ...ys: int) -> int {
    base - seed
}

export let sum = sumImpl
export let pack = packImpl
"#,
        )],
    );
    assert_eq!(
        non_catch_all_variadic_tail_mismatched_match_pipe_alias.code(),
        "TPZ6PY0001"
    );
    match non_catch_all_variadic_tail_mismatched_match_pipe_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "pipe stage call target",
                "non-catch-all variadic-tail mismatched match namespace member pipe alias"
            )
        }
        other => {
            panic!(
                "non-catch-all variadic-tail mismatched match namespace member pipe alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let non_catch_all_returning_match_pipe_alias = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let seeds = [1, 2]
    let f = match seeds[0] {
        case n if n == 1 => return 0
        case n if n == 2 => util.add
    }
    5 |> f(a: _)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}
export let add = addImpl
"#,
        )],
    );
    assert_eq!(
        non_catch_all_returning_match_pipe_alias.code(),
        "TPZ6PY0001"
    );
    match non_catch_all_returning_match_pipe_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "pipe stage call target",
                "non-catch-all returning match namespace member pipe alias"
            )
        }
        other => {
            panic!(
                "non-catch-all returning match namespace member pipe alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let mismatched_match_callable_alias = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let f = match true {
        case true => util.add
        case _ => util.mul
    }
    f(a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function mulImpl(x: int, y: int) -> int {
    x * y
}

export let add = addImpl
export let mul = mulImpl
"#,
        )],
    );
    assert_eq!(mismatched_match_callable_alias.code(), "TPZ6PY0001");
    match mismatched_match_callable_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call target",
                "mismatched match namespace member callable alias"
            )
        }
        other => {
            panic!(
                "mismatched match namespace member callable alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let default_presence_mismatched_match_callable_alias = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let f = match true {
        case true => util.add
        case _ => util.sub
    }
    f(a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function subImpl(a: int, b: int) -> int {
    a - b
}

export let add = addImpl
export let sub = subImpl
"#,
        )],
    );
    assert_eq!(
        default_presence_mismatched_match_callable_alias.code(),
        "TPZ6PY0001"
    );
    match default_presence_mismatched_match_callable_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call target",
                "default-presence mismatched match namespace member callable alias"
            )
        }
        other => {
            panic!(
                "default-presence mismatched match namespace member callable alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let returning_match_callable_alias = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let f = match true {
        case true => return 0
        case _ => util.add
    }
    f(a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}
export let add = addImpl
"#,
        )],
    );
    assert_eq!(returning_match_callable_alias.code(), "TPZ6PY0001");
    match returning_match_callable_alias.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call target",
                "returning match namespace member callable alias"
            )
        }
        other => {
            panic!(
                "returning match namespace member callable alias: expected unsupported error, got {other:?}"
            )
        }
    }

    let mismatched_conditional_hof_alias = emit_source_with_files(
        r#"
import util
let callbacks = if true {
    util.fastCallbacks
} else {
    util.slowCallbacks
}
concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[0])
    }
    boom: {
        let ys = [1]
        ys[2]
    }
}
0
"#,
        &[(
            "util.tpz",
            r#"
function fast(x: int) -> int {
    x
}

function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

export let fastCallbacks = [fast]
export let slowCallbacks = [spin]
"#,
        )],
    );
    assert!(
        mismatched_conditional_hof_alias.contains("yield from tpz_array_map__co("),
        "mismatched conditional namespace-member HOF aliases should use the runtime cooperative driver: {mismatched_conditional_hof_alias}"
    );
    assert_generated_python_gates(&mismatched_conditional_hof_alias).unwrap_or_else(|e| {
        panic!("mismatched conditional namespace-member HOF alias Python gate failed: {e}")
    });

    let statementful_conditional_hof_alias = emit_source_with_files(
        r#"
import util
let callbacks = if true {
    let marker = 1
    util.callbacks
} else {
    util.callbacks
}
concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[0])
    }
    boom: {
        let ys = [1]
        ys[2]
    }
}
0
"#,
        &[(
            "util.tpz",
            r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
export let callbacks = [spin]
"#,
        )],
    );
    assert!(
        statementful_conditional_hof_alias.contains("yield from tpz_array_map__co(")
            && !statementful_conditional_hof_alias
                .contains("_tpz_mod__t_7574696c___t_7370696e__co(host, __tpz_cb_0)"),
        "statementful conditional namespace-member HOF aliases should stay on the runtime cooperative driver without direct static callback recovery: {statementful_conditional_hof_alias}"
    );
    assert_generated_python_gates(&statementful_conditional_hof_alias).unwrap_or_else(|e| {
        panic!("statementful conditional namespace-member HOF alias Python gate failed: {e}")
    });

    let statementful_mismatched_conditional_hof_alias = emit_source_with_files(
        r#"
import util
let seeds = [1]
let callbacks = if seeds[0] == 1 {
    let marker = 1
    util.fastCallbacks
} else {
    util.slowCallbacks
}
let result = concurrent {
    value: {
        let xs = [1]
        let ys = xs.map(callbacks[0])
        ys[0]
    }
    idle: 0
}
result.value
"#,
        &[(
            "util.tpz",
            r#"
function fast(x: int) -> int {
    x + 40
}
function spin(x: int) -> int {
    1 / 0
}
export let fastCallbacks = [fast]
export let slowCallbacks = [spin]
"#,
        )],
    );
    assert!(
        statementful_mismatched_conditional_hof_alias.contains("yield from tpz_array_map__co(")
            && !statementful_mismatched_conditional_hof_alias
                .contains("_tpz_mod__t_7574696c___t_66617374__co(host, __tpz_cb_0)")
            && !statementful_mismatched_conditional_hof_alias
                .contains("_tpz_mod__t_7574696c___t_7370696e__co(host, __tpz_cb_0)"),
        "statementful mismatched conditional namespace-member HOF aliases should stay on the runtime cooperative driver without direct static callback recovery: {statementful_mismatched_conditional_hof_alias}"
    );
    assert_generated_python_ok_int(
        &statementful_mismatched_conditional_hof_alias,
        41,
        "statementful mismatched conditional namespace-member HOF callback runtime-driver parity",
    );

    let mutable_alias_chain_after_reassignment = emit_source_with_files(
        r#"
import util
let base = util.callbacks
let mut callbacks = base
callbacks = util.callbacks
concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[1])
    }
    boom: {
        let ys = [1]
        ys[2]
    }
}
0
"#,
        &[(
            "util.tpz",
            r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
export let callbacks = [42, spin]
"#,
        )],
    );
    assert!(
        mutable_alias_chain_after_reassignment.contains("yield from tpz_array_map__co(")
            && mutable_alias_chain_after_reassignment
                .contains("_tpz_mod__t_7574696c___t_7370696e__co(host, __tpz_cb_0)"),
        "reassigned mutable namespace member alias chains should use assignment-point RHS cooperative metadata: {mutable_alias_chain_after_reassignment}"
    );
    assert_generated_python_gates(&mutable_alias_chain_after_reassignment).unwrap_or_else(|e| {
        panic!("reassigned mutable namespace member alias chain Python gate failed: {e}")
    });

    let mutable_assignment = emit_source_with_files(
        r#"
import util
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
let mut callbacks = [42, spin]
callbacks = util.callbacks
concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[1])
    }
    boom: {
        let ys = [1]
        ys[2]
    }
}
0
"#,
        &[(
            "util.tpz",
            r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
export let callbacks = [42, spin]
"#,
        )],
    );
    assert!(
        mutable_assignment.contains("yield from tpz_array_map__co(")
            && mutable_assignment
                .contains("_tpz_mod__t_7574696c___t_7370696e__co(host, __tpz_cb_0)"),
        "mutable assignment from namespace members should use assignment-point RHS cooperative metadata: {mutable_assignment}"
    );
    assert_generated_python_gates(&mutable_assignment)
        .unwrap_or_else(|e| panic!("mutable namespace member assignment Python gate failed: {e}"));

    let mutable_match_reassignment_direct_call = emit_source_with_files(
        r#"
import util
function main() -> int {
    let mut f = util.add
    f = match true {
        case true => util.sub
        case false => util.add
    }
    f(a: 5) + 100
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function addImpl(a: int, b: int = 2) -> int {
    a + b
}

function subImpl(a: int, b: int = 100) -> int {
    a - b
}

export let add = addImpl
export let sub = subImpl
"#,
        )],
    );
    assert!(
        mutable_match_reassignment_direct_call.contains("(_t_61=5)"),
        "mutable reassignment from non-catch-all match namespace-member aliases should keep named direct-call shape: {mutable_match_reassignment_direct_call}"
    );
    assert_generated_python_ok_int(
        &mutable_match_reassignment_direct_call,
        5,
        "mutable reassigned non-catch-all match namespace-member alias direct-call parity",
    );

    let stale_initializer_after_reassignment = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let mut callbacks = util.addCallbacks
    callbacks = util.mulCallbacks
    callbacks[0](a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function mul(x: int, y: int) -> int {
    x * y
}
export let addCallbacks = [add]
export let mulCallbacks = [mul]
"#,
        )],
    );
    assert_eq!(stale_initializer_after_reassignment.code(), "TPZ6PY0001");
    match stale_initializer_after_reassignment.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call argument shape",
                "stale initializer metadata after reassignment"
            )
        }
        other => {
            panic!(
                "stale initializer metadata after reassignment: expected unsupported error, got {other:?}"
            )
        }
    }

    let conditional_reassignment_barrier = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let mut callbacks = util.addCallbacks
    if true {
        callbacks = util.mulCallbacks
    }
    callbacks[0](x: 5, y: 6)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function mul(x: int, y: int) -> int {
    x * y
}
export let addCallbacks = [add]
export let mulCallbacks = [mul]
"#,
        )],
    );
    assert_eq!(conditional_reassignment_barrier.code(), "TPZ6PY0001");
    match conditional_reassignment_barrier.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call argument shape",
                "conditional mutable storage reassignment barrier"
            )
        }
        other => {
            panic!(
                "conditional mutable storage reassignment barrier: expected unsupported error, got {other:?}"
            )
        }
    }

    let loop_reassignment_barrier = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let mut callbacks = util.addCallbacks
    while false {
        callbacks = util.mulCallbacks
    }
    callbacks[0](a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function mul(x: int, y: int) -> int {
    x * y
}
export let addCallbacks = [add]
export let mulCallbacks = [mul]
"#,
        )],
    );
    assert_eq!(loop_reassignment_barrier.code(), "TPZ6PY0001");
    match loop_reassignment_barrier.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call argument shape",
                "loop mutable storage reassignment barrier"
            )
        }
        other => {
            panic!(
                "loop mutable storage reassignment barrier: expected unsupported error, got {other:?}"
            )
        }
    }

    let mutable_source_reassignment = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let mut source = util.mulCallbacks
    let mut callbacks = util.addCallbacks
    callbacks = source
    callbacks[0](x: 5, y: 6)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
function mul(x: int, y: int) -> int {
    x * y
}
export let addCallbacks = [add]
export let mulCallbacks = [mul]
"#,
        )],
    );
    assert_eq!(mutable_source_reassignment.code(), "TPZ6PY0001");
    match mutable_source_reassignment.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call argument shape",
                "mutable source storage reassignment"
            )
        }
        other => {
            panic!("mutable source storage reassignment: expected unsupported error, got {other:?}")
        }
    }

    let nested_record_reassignment = emit_error_for_source_with_files(
        r#"
import util
function main() -> int {
    let mut handlers = util.handlers
    handlers = util.nestedHandlers
    handlers.nested.primary(a: 5)
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
export let handlers = { primary: add }
export let nestedHandlers = { nested: { primary: add } }
"#,
        )],
    );
    assert_eq!(nested_record_reassignment.code(), "TPZ6PY0001");
    match nested_record_reassignment.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(
                what, "call argument shape",
                "nested record storage reassignment direct-only boundary"
            )
        }
        other => {
            panic!(
                "nested record storage reassignment direct-only boundary: expected unsupported error, got {other:?}"
            )
        }
    }

    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", "import util\n0\n");
    provider.add_file(
        "util.tpz",
        r#"
function add(a: int, b: int = 2) -> int {
    a + b
}
export let mut callbacks = [add]
"#,
    );
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    assert!(
        !unit.diagnostics.is_empty(),
        "mutable exported value bindings must be rejected before Python metadata is considered"
    );
}
