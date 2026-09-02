use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir {
    root: PathBuf,
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn temp_dir(name: &str) -> TempDir {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("topaz-cli-{name}-{}-{nanos}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create temp root");
    TempDir { root }
}

fn write_file(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }
    fs::write(path, text).expect("write temp source");
}

#[test]
fn build_python_target_rejects_concrete_non_callable_local_calls_before_emission() {
    let cases = [
        (
            "zero_arg",
            r#"
function main() -> int {
    let f = 1
    f()
}
"#,
        ),
        (
            "positional",
            r#"
function main() -> int {
    let f = 1
    f(1)
}
"#,
        ),
    ];

    for (name, source) in cases {
        let temp = temp_dir(&format!("python-not-callable-{name}"));
        let entry = temp.root.join("main.tpz");
        let out_dir = temp.root.join("out");
        write_file(&entry, source);

        let entry_arg = entry.to_string_lossy().into_owned();
        let out_arg = out_dir.to_string_lossy().into_owned();
        assert_eq!(
            build_entry(
                &entry_arg,
                None,
                Some(&out_arg),
                false,
                false,
                LangVersion::V5_5,
                false,
                Backend::Native,
                BuildTarget::Python,
                false,
                &[],
                None,
            ),
            ExitCode::FAILURE,
            "{name}: Python target build must stop at the shared checker"
        );
        assert!(
            !out_dir.join("program.py").exists(),
            "{name}: Python artifact must not be written after a checker rejection"
        );
        assert!(
            !out_dir.join("topaz_py_rt.py").exists(),
            "{name}: Python runtime artifact must not be written after a checker rejection"
        );

        let (base, entry_rel, root_rel) =
            split_absolute(&entry_arg, None).expect("absolute temp entry splits");
        let provider = PhysicalProvider::new(base);
        let resolved = resolve_with_version(
            &provider,
            &entry_rel,
            root_rel.as_deref(),
            LangVersion::V5_5,
        );
        assert!(
            resolved.diagnostics.is_empty(),
            "{name}: source should resolve before checker rejection: {:?}",
            resolved.diagnostics
        );
        let modules = unit_modules(&resolved);
        let checked = topaz_check::check_unit_with_version(&modules, LangVersion::V5_5);
        assert_eq!(
            checked.diagnostics.len(),
            1,
            "{name}: expected exactly one checker diagnostic: {:?}",
            checked.diagnostics
        );
        let diagnostic = &checked.diagnostics[0];
        assert_eq!(diagnostic.code, topaz_check::codes::NOT_CALLABLE, "{name}");
        assert!(
            diagnostic.message.contains("not callable"),
            "{name}: diagnostic should explain callability: {:?}",
            diagnostic
        );
        let span = diagnostic.primary.span;
        let file = resolved.map.file(span.file);
        let text = &file.src()[span.lo as usize..span.hi as usize];
        assert_eq!(text, "f", "{name}: primary span must point at the callee");
    }
}

#[test]
fn build_python_target_rejects_dynamic_index_order_faults_before_emission() {
    let prefix = r#"
function add(a: int = 0, b: int = 0, ...xs: int) -> int {
    a + b + xs.length
}
function addRequired(a: int, b: int, ...xs: int) -> int {
    a + b + xs.length
}
function pick(label: string) -> int {
    0
}
"#;
    let cases = [
        (
            "direct_positional_after_named",
            format!(
                "{prefix}
function main() -> int {{
    let arr = [add]
    arr[pick(\"k\")](a: 1, 2)
}}
"
            ),
            "positional arguments may not follow named arguments",
        ),
        (
            "pipe_positional_after_named",
            format!(
                "{prefix}
function main() -> int {{
    let arr = [add]
    1 |> arr[pick(\"k\")](a: _, 2)
}}
"
            ),
            "positional arguments may not follow named arguments",
        ),
        (
            "direct_named_before_spread",
            format!(
                "{prefix}
function main() -> int {{
    let arr = [add]
    arr[pick(\"k\")](a: 1, ...[2])
}}
"
            ),
            "named arguments must follow spread arguments",
        ),
        (
            "pipe_named_before_spread",
            format!(
                "{prefix}
function main() -> int {{
    let arr = [add]
    1 |> arr[pick(\"k\")](a: _, ...[2])
}}
"
            ),
            "named arguments must follow spread arguments",
        ),
        (
            "direct_spread_skips_required",
            format!(
                "{prefix}
function main() -> int {{
    let arr = [addRequired]
    arr[pick(\"k\")](...[1, 2])
}}
"
            ),
            "a spread argument cannot skip an unsatisfied fixed parameter",
        ),
        (
            "pipe_spread_skips_required",
            format!(
                "{prefix}
function main() -> int {{
    let arr = [addRequired]
    1 |> arr[pick(\"k\")](...[2])
}}
"
            ),
            "a spread argument cannot skip an unsatisfied fixed parameter",
        ),
    ];

    for (name, source, message_fragment) in cases {
        let temp = temp_dir(&format!("python-dynamic-index-order-fault-{name}"));
        let entry = temp.root.join("main.tpz");
        let out_dir = temp.root.join("out");
        write_file(&entry, &source);

        let entry_arg = entry.to_string_lossy().into_owned();
        let out_arg = out_dir.to_string_lossy().into_owned();
        assert_eq!(
            build_entry(
                &entry_arg,
                None,
                Some(&out_arg),
                false,
                false,
                LangVersion::V5_5,
                false,
                Backend::Native,
                BuildTarget::Python,
                false,
                &[],
                None,
            ),
            ExitCode::FAILURE,
            "{name}: Python target build must stop at the shared checker"
        );
        assert!(
            !out_dir.join("program.py").exists(),
            "{name}: Python artifact must not be written after a checker rejection"
        );
        assert!(
            !out_dir.join("topaz_py_rt.py").exists(),
            "{name}: Python runtime artifact must not be written after a checker rejection"
        );

        let (base, entry_rel, root_rel) =
            split_absolute(&entry_arg, None).expect("absolute temp entry splits");
        let provider = PhysicalProvider::new(base);
        let resolved = resolve_with_version(
            &provider,
            &entry_rel,
            root_rel.as_deref(),
            LangVersion::V5_5,
        );
        assert!(
            resolved.diagnostics.is_empty(),
            "{name}: source should resolve before checker rejection: {:?}",
            resolved.diagnostics
        );
        let modules = unit_modules(&resolved);
        let checked = topaz_check::check_unit_with_version(&modules, LangVersion::V5_5);
        assert!(
            checked.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == topaz_check::codes::ARITY
                    && diagnostic.message.contains(message_fragment)
            }),
            "{name}: expected TPZ5004 containing {message_fragment:?}, got {:?}",
            checked.diagnostics
        );
    }
}

#[test]
fn build_python_target_declines_faulting_function_defaults_without_artifact() {
    let cases = [
        (
            "faulting_const_default",
            r#"
function score(x: int = 1 / 0) -> int {
    x
}
score()
"#,
            "1 / 0",
            "integer division by zero",
        ),
        (
            "overflowing_const_default",
            r#"
function score(x: int = 9223372036854775807 + 1) -> int {
    x
}
score()
"#,
            "9223372036854775807 + 1",
            "integer addition overflows",
        ),
    ];

    for (name, source, default_expr, message_fragment) in cases {
        let temp = temp_dir(&format!("python-non-const-function-default-{name}"));
        let entry = temp.root.join("main.tpz");
        let out_dir = temp.root.join("out");
        write_file(&entry, source);

        let entry_arg = entry.to_string_lossy().into_owned();
        let out_arg = out_dir.to_string_lossy().into_owned();
        assert_eq!(
            build_entry(
                &entry_arg,
                None,
                Some(&out_arg),
                false,
                false,
                LangVersion::V5_5,
                false,
                Backend::Native,
                BuildTarget::Python,
                false,
                &[],
                None,
            ),
            ExitCode::FAILURE,
            "{name}: Python target build must stop at the shared checker"
        );
        assert!(
            !out_dir.join("program.py").exists(),
            "{name}: Python artifact must not be written after a checker rejection"
        );
        assert!(
            !out_dir.join("topaz_py_rt.py").exists(),
            "{name}: Python runtime artifact must not be written after a checker rejection"
        );

        let (base, entry_rel, root_rel) =
            split_absolute(&entry_arg, None).expect("absolute temp entry splits");
        let provider = PhysicalProvider::new(base);
        let resolved = resolve_with_version(
            &provider,
            &entry_rel,
            root_rel.as_deref(),
            LangVersion::V5_5,
        );
        assert!(
            resolved.diagnostics.is_empty(),
            "{name}: source should resolve before checker rejection: {:?}",
            resolved.diagnostics
        );
        let modules = unit_modules(&resolved);
        let checked = topaz_check::check_unit_with_version(&modules, LangVersion::V5_5);
        assert_eq!(
            checked.diagnostics.len(),
            1,
            "{name}: expected exactly one checker diagnostic: {:?}",
            checked.diagnostics
        );
        let diagnostic = &checked.diagnostics[0];
        assert_eq!(diagnostic.code, topaz_check::codes::TYPE_MISMATCH, "{name}");
        assert!(
            diagnostic.message.contains(message_fragment),
            "{name}: diagnostic should explain the const-expression fault: {:?}",
            diagnostic
        );
        let span = diagnostic.primary.span;
        let file = resolved.map.file(span.file);
        let text = &file.src()[span.lo as usize..span.hi as usize];
        assert_eq!(
            text, default_expr,
            "{name}: primary span must point at the default expression"
        );
    }
}

#[test]
fn build_python_target_declines_non_const_function_defaults_shapes_without_artifact() {
    let cases = [
        (
            "nested_call_default",
            r#"
function main() -> int {
    function first(x: int = second()) -> int { x }
    function second() -> int { 1 }
    first()
}
main()
"#,
            "second()",
            "constant expressions",
        ),
        (
            "record_default",
            r#"
record Point { x: int, y: int }
function score(p: Point = Point { x: 2, y: 3 }) -> int {
    p.x * 10 + p.y
}
score()
"#,
            "Point { x: 2, y: 3 }",
            "constant expressions",
        ),
        (
            "call_default",
            r#"
function seed() -> int {
    7
}
function score(x: int = seed()) -> int {
    x
}
score()
"#,
            "seed()",
            "constant expressions",
        ),
        (
            "enum_default",
            r#"
enum Color derives Eq, Order, Show { Red, Blue }
function score(c: Color = Color.Blue) -> string {
    "{c}"
}
score()
"#,
            "Color.Blue",
            "constant expressions",
        ),
    ];

    for (name, source, default_expr, message_fragment) in cases {
        let temp = temp_dir(&format!("python-non-const-function-default-shape-{name}"));
        let entry = temp.root.join("main.tpz");
        let out_dir = temp.root.join("out");
        write_file(&entry, source);

        let entry_arg = entry.to_string_lossy().into_owned();
        let out_arg = out_dir.to_string_lossy().into_owned();
        assert_eq!(
            build_entry(
                &entry_arg,
                None,
                Some(&out_arg),
                false,
                false,
                LangVersion::V5_5,
                false,
                Backend::Native,
                BuildTarget::Python,
                false,
                &[],
                None,
            ),
            ExitCode::FAILURE,
            "{name}: Python target build must stop at the shared checker"
        );
        assert!(
            !out_dir.join("program.py").exists(),
            "{name}: Python artifact must not be written after a checker rejection"
        );
        assert!(
            !out_dir.join("topaz_py_rt.py").exists(),
            "{name}: Python runtime artifact must not be written after a checker rejection"
        );

        let (base, entry_rel, root_rel) =
            split_absolute(&entry_arg, None).expect("absolute temp entry splits");
        let provider = PhysicalProvider::new(base);
        let resolved = resolve_with_version(
            &provider,
            &entry_rel,
            root_rel.as_deref(),
            LangVersion::V5_5,
        );
        assert!(
            resolved.diagnostics.is_empty(),
            "{name}: source should resolve before checker rejection: {:?}",
            resolved.diagnostics
        );
        let modules = unit_modules(&resolved);
        let checked = topaz_check::check_unit_with_version(&modules, LangVersion::V5_5);
        assert_eq!(
            checked.diagnostics.len(),
            1,
            "{name}: expected exactly one checker diagnostic: {:?}",
            checked.diagnostics
        );
        let diagnostic = &checked.diagnostics[0];
        assert_eq!(diagnostic.code, topaz_check::codes::TYPE_MISMATCH, "{name}");
        assert!(
            diagnostic.message.contains(message_fragment),
            "{name}: diagnostic should explain the const-expression boundary: {:?}",
            diagnostic
        );
        let span = diagnostic.primary.span;
        let file = resolved.map.file(span.file);
        let text = &file.src()[span.lo as usize..span.hi as usize];
        assert_eq!(
            text, default_expr,
            "{name}: primary span must point at the default expression"
        );
    }
}

#[test]
fn build_python_target_accepts_identifier_function_defaults() {
    let cases = [
        (
            "const_identifier_default",
            r#"
const A = 3
function score(x: int = A) -> int {
    x
}
score()
"#,
        ),
        (
            "let_identifier_default",
            r#"
let A = 3
function score(x: int = A) -> int {
    x
}
score()
"#,
        ),
        (
            "shadowed_parameter_default_uses_defining_scope",
            r#"
let A = 10
function score(A: int, x: int = A) -> int {
    x
}
score(99)
"#,
        ),
    ];

    for (name, source) in cases {
        let temp = temp_dir(&format!("python-identifier-function-default-{name}"));
        let entry = temp.root.join("main.tpz");
        let out_dir = temp.root.join("out");
        write_file(&entry, source);

        let entry_arg = entry.to_string_lossy().into_owned();
        let out_arg = out_dir.to_string_lossy().into_owned();
        let (base, entry_rel, root_rel) =
            split_absolute(&entry_arg, None).expect("absolute temp entry splits");
        let provider = PhysicalProvider::new(base);
        let resolved = resolve_with_version(
            &provider,
            &entry_rel,
            root_rel.as_deref(),
            LangVersion::V5_5,
        );
        assert!(
            resolved.diagnostics.is_empty(),
            "{name}: source should resolve cleanly: {:?}",
            resolved.diagnostics
        );
        let modules = unit_modules(&resolved);
        let checked = topaz_check::check_unit_with_version(&modules, LangVersion::V5_5);
        assert!(
            checked.diagnostics.is_empty(),
            "{name}: identifier defaults are accepted surface, not checker gates: {:?}",
            checked.diagnostics
        );
        assert_eq!(
            build_entry(
                &entry_arg,
                None,
                Some(&out_arg),
                false,
                false,
                LangVersion::V5_5,
                false,
                Backend::Native,
                BuildTarget::Python,
                false,
                &[],
                None,
            ),
            ExitCode::SUCCESS,
            "{name}: Python target should emit identifier function defaults"
        );
        assert!(
            out_dir.join("program.py").exists(),
            "{name}: Python artifact should be written"
        );
        assert!(
            out_dir.join("topaz_py_rt.py").exists(),
            "{name}: Python runtime artifact should be written"
        );
    }
}

#[test]
fn build_python_target_rejects_immutable_assignment_before_emission() {
    let source = r#"
function main() -> int {
    let x = 1
    x = 2
    x
}
main()
"#;
    let temp = temp_dir("python-immutable-assignment-checker-gate");
    let entry = temp.root.join("main.tpz");
    let out_dir = temp.root.join("out");
    write_file(&entry, source);
    let entry_arg = entry.to_string_lossy().into_owned();
    let out_arg = out_dir.to_string_lossy().into_owned();

    assert_eq!(
        build_entry(
            &entry_arg,
            None,
            Some(&out_arg),
            false,
            false,
            LangVersion::V5_5,
            false,
            Backend::Native,
            BuildTarget::Python,
            false,
            &[],
            None,
        ),
        ExitCode::FAILURE
    );
    assert!(!out_dir.join("program.py").exists());
    assert!(!out_dir.join("topaz_py_rt.py").exists());

    let (base, entry_rel, root_rel) =
        split_absolute(&entry_arg, None).expect("absolute temp entry splits");
    let provider = PhysicalProvider::new(base);
    let resolved = resolve_with_version(
        &provider,
        &entry_rel,
        root_rel.as_deref(),
        LangVersion::V5_5,
    );
    assert!(
        resolved.diagnostics.is_empty(),
        "{:?}",
        resolved.diagnostics
    );
    let modules = unit_modules(&resolved);
    let checked = topaz_check::check_unit_with_version(&modules, LangVersion::V5_5);
    assert_eq!(checked.diagnostics.len(), 1, "{:?}", checked.diagnostics);
    assert_eq!(checked.diagnostics[0].code, topaz_check::codes::IMMUTABLE);
    let span = checked.diagnostics[0].primary.span;
    let file = resolved.map.file(span.file);
    assert_eq!(&file.src()[span.lo as usize..span.hi as usize], "x");
}
