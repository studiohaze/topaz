//! v5.4 EXPLICIT call-site type-argument check witnesses — `f<T>(args)`,
//! `Map.new<K, V>()`, `Set.of<T>()`. These are CHECK-ONLY: the type-args
//! seed the callee scheme's type variables (ground truth), surfacing a
//! conflicting argument as the ordinary `expect` mismatch and typing an
//! otherwise-unsolvable empty collection without an annotation. The call
//! lowers type-erased, so the run≡build identity is locked by the
//! difftest pair `call_type_args_explicit` / `call_type_args_inferred`.

use topaz_check::check_program_with_version;
use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;

fn check(src: &str) -> Vec<String> {
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_4,
        },
    );
    assert!(out.diagnostics.is_empty(), "test source must parse: {src}");
    check_program_with_version(src, &out.program, LangVersion::V5_4)
        .diagnostics
        .iter()
        .map(|d| format!("{} {}", d.code.as_str(), d.message))
        .collect()
}

fn assert_clean(src: &str) {
    let diags = check(src);
    assert!(diags.is_empty(), "expected clean, got: {diags:?}");
}

fn assert_code(src: &str, code: &str) {
    let diags = check(src);
    assert!(
        diags.iter().any(|d| d.starts_with(code)),
        "expected {code}, got: {diags:?}"
    );
}

const FIRST: &str = "function first<T>(xs: Array<T>) -> T {\n    return xs[0]\n}\n";

// ---- positive: explicit type-args type and agree with the args ------

#[test]
fn explicit_type_arg_on_free_generic_function_is_clean() {
    // `first<int>([..])` seeds T = int; the int array argument agrees.
    assert_clean(&format!(
        "{FIRST}let a: int = first<int>([1, 2, 3])\nprint(\"{{a}}\")"
    ));
}

#[test]
fn explicit_type_arg_makes_a_bare_binding_solvable() {
    // Without the explicit arg, a BARE `let` cannot solve T (TPZ5020); the
    // explicit `<int>` makes it ground, so the bare binding now types.
    assert_clean(&format!(
        "{FIRST}let a = first<int>([1, 2, 3])\nprint(\"{{a}}\")"
    ));
}

#[test]
fn explicit_type_args_on_generic_static_members_type_without_annotation() {
    // `Map.new<K, V>()` / `Set.of<T>()` are fully typed from the type-args.
    assert_clean("let m = Map.new<string, int>()\nprint(\"{m.length}\")");
    assert_clean("let m = Map.ofEntries<string, int>([])\nprint(\"{m.length}\")");
    assert_clean("let s = Set.of<int>()\nprint(\"{s.length}\")");
}

#[test]
fn explicit_type_args_on_shipped_generic_receiver_members_are_clean() {
    assert_clean(
        "let xs: Array<int> = [1, 2]\n\
         let mapped: Array<string> = xs.map<string>((x: int) => \"{x}\")\n\
         let sorted: Array<int> = xs.sortedBy<int>((x: int) => x)\n\
         let opt: Option<int> = Some(1)\n\
         let ok: Result<int, string> = opt.okOr<string>(\"missing\")\n\
         print(\"{mapped.length}:{sorted.length}:{ok}\")",
    );
}

#[test]
fn explicit_type_args_on_generic_receiver_pipe_stage_are_clean() {
    assert_clean(
        "let xs: Array<int> = [1, 2]\n\
         let mapped: Array<string> = ((x: int) => \"{x}\") |> xs.map<string>()\n\
         print(mapped.join(\",\"))",
    );
}

#[test]
fn explicit_type_args_follow_a_local_generic_alias() {
    assert_clean(&format!(
        "{FIRST}let alias = first\nlet a: int = alias<int>([1, 2])\nprint(\"{{a}}\")"
    ));
}

#[test]
fn explicit_type_arg_with_gtgt_split_is_clean() {
    // `f<Array<int>>(..)` — the `>>` close splits; T = Array<int>.
    assert_clean(
        "function firstArr<T>(xs: Array<Array<T>>) -> Array<T> {\n    return xs[0]\n}\n\
         let a: Array<int> = firstArr<int>([[1, 2], [3]])\nprint(\"{a[0]}\")",
    );
}

// ---- negative: explicit arg conflicts / arity / non-generic ---------

#[test]
fn explicit_type_arg_conflicting_with_the_argument_is_a_mismatch() {
    // `first<string>([1,2,3])`: the explicit `string` is ground truth, so the
    // int array argument surfaces the ordinary TYPE_MISMATCH (TPZ5001).
    assert_code(
        &format!("{FIRST}let a = first<string>([1, 2, 3])\nprint(\"{{a}}\")"),
        "TPZ5001",
    );
}

#[test]
fn explicit_type_arg_on_iterable_fixup_is_not_overwritten() {
    // `filter<string>(Array<int>, ...)`: the explicit `string` seed for T must
    // not be clobbered by the iterable element `int`.
    assert_code(
        "let xs = filter<string>([1, 2, 3], (x) => true)\nprint(\"{xs.length}\")",
        "TPZ5001",
    );
}

#[test]
fn explicit_type_arg_on_pipeline_iterable_fixup_is_not_overwritten() {
    // Same seed-preservation rule through the §11 leading-argument path.
    assert_code(
        "let xs = [1, 2, 3] |> filter<string>((x) => true)\nprint(\"{xs.length}\")",
        "TPZ5001",
    );
}

#[test]
fn too_many_type_args_is_tpz5510() {
    assert_code(
        &format!("{FIRST}let a = first<int, int>([1, 2, 3])\nprint(\"{{a}}\")"),
        "TPZ5510",
    );
}

#[test]
fn type_args_on_a_non_generic_callee_is_tpz5512() {
    assert_code(
        "function f(x: int) -> int {\n    return x\n}\nlet a = f<int>(3)\nprint(\"{a}\")",
        "TPZ5512",
    );
}

#[test]
fn type_args_on_an_optional_call_are_tpz5512() {
    assert_code(
        "let xs: Option<Array<int>> = Some([1])\nlet n = xs?.get<int>(0)",
        "TPZ5512",
    );
}

#[test]
fn type_args_on_newtype_constructor_are_tpz5512() {
    assert_code(
        "newtype UserId = int\nlet id = UserId<int>(1)\nprint(\"{id}\")",
        "TPZ5512",
    );
}

#[test]
fn type_args_on_enum_constructor_are_tpz5512() {
    assert_code(
        "enum Status { Done(int) }\nlet s = Status.Done<int>(1)\nprint(\"{s}\")",
        "TPZ5512",
    );
}

#[test]
fn type_args_on_inherent_method_are_tpz5512() {
    assert_code(
        "record P { x: int }\nimpl P { function get(self) -> int { self.x } }\nlet p = P { x: 1 }\nlet x = p.get<int>()\nprint(\"{x}\")",
        "TPZ5512",
    );
}

#[test]
fn type_args_on_callable_record_field_are_tpz5512() {
    assert_code(
        "record Holder { f: (int) -> int }\nlet h = Holder { f: (x: int) => x }\nlet x = h.f<int>(1)\nprint(\"{x}\")",
        "TPZ5512",
    );
}

#[test]
fn type_args_on_protocol_static_dispatch_are_tpz5512() {
    assert_code(
        "record User derives Show { name: string }\nlet u = User { name: \"Ada\" }\nlet s = Show.show<User>(u)\nprint(s)",
        "TPZ5512",
    );
}

#[test]
fn type_args_on_monomorphic_receiver_member_are_tpz5512() {
    assert_code(
        "let xs: Array<int> = [1, 2]\nlet ys = xs.filter<int>((x: int) => true)\nprint(\"{ys.length}\")",
        "TPZ5512",
    );
}
