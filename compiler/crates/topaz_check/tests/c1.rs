//! Phase C-1 witnesses: type formation, §3 well-formedness
//! diagnostics, and the CDR-004 §3 subtyping rules (including the
//! variance counterexamples that fixed the design).

use topaz_check::{Ctor, Lit, Prim, Type, check_program, is_subtype};
use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;

fn check(src: &str) -> Vec<String> {
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_2,
        },
    );
    assert!(out.diagnostics.is_empty(), "test source must parse: {src}");
    check_program(src, &out.program)
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

// ---- formation: positive --------------------------------------------

#[test]
fn well_formed_types_are_clean() {
    assert_clean("type Status = \"open\" | \"closed\"");
    assert_clean("type Port = int");
    assert_clean("type Handler = (string, int) -> bool");
    assert_clean("type Tail = (...int) -> ()");
    assert_clean("type Pair<A, B> = { first: A, second: B }");
    assert_clean("type Nullable<T> = T | null");
    assert_clean("type Lookup = Map<string, Array<int>>");
    assert_clean("let port: int | null = null\nprint(\"{port}\")");
    assert_clean("function f(xs: Array<int>) -> int {\n    return 0\n}");
    assert_clean("function g<T>(x: T) -> T {\n    return x\n}");
    assert_clean(
        "function h(pick: (int) -> bool) -> () {\n    let inner = (x: int) => pick(x)\n    print(\"{inner(1)}\")\n}",
    );
}

#[test]
fn alias_chains_resolve_structurally() {
    assert_clean("type A = int\ntype B = A | string\ntype C = Array<B>");
}

// ---- formation: §3 diagnostics --------------------------------------

#[test]
fn alias_cycle_is_tpz5023() {
    assert_code("type A = B\ntype B = A", "TPZ5023");
    assert_code("type Loop = Array<Loop>", "TPZ5023");
}

#[test]
fn duplicate_alias_is_tpz5008() {
    assert_code("type A = int\ntype A = string", "TPZ5008");
}

#[test]
fn unknown_and_malformed_names_are_tpz5022() {
    // Undeclared type names form as opaque ambient types (the same
    // posture the interpreter takes with ambient value names) until
    // module-aware checking; misuses of KNOWN names still diagnose.
    assert_clean("type T = NoSuchType");
    assert_code("type T = Array<int, int>", "TPZ5022");
    assert_code("type T = Result<int>", "TPZ5022");
    assert_code("type T = int<string>", "TPZ5022");
    assert_code("type Dup = { x: int, x: string }", "TPZ5022");
    assert_code("type P<T, T> = T", "TPZ5022");
    assert_code("type W<T> = Array<T<int>>", "TPZ5022");
}

#[test]
fn variadic_must_be_final_tpz5024() {
    assert_code("type Bad = (...int, string) -> ()", "TPZ5024");
}

#[test]
fn annotations_inside_bodies_are_walked() {
    // The offending type sits inside a lambda annotation inside a
    // function body — the walker must reach it.
    assert_code(
        "function f() -> () {\n    let g = (x: Array<int, int>) => x\n    print(\"{g(1)}\")\n}",
        "TPZ5022",
    );
    // ... and inside a typed match pattern.
    assert_code(
        "function f(v: int) -> int {\n    return match v {\n        case n: Result<int> => 0\n        case _ => 1\n    }\n}",
        "TPZ5022",
    );
}

#[test]
fn function_declaration_constraints() {
    assert_code(
        "function f<T, T>(x: T) -> T {
    return x
}",
        "TPZ5022",
    );
    assert_code(
        "function f(...xs: int, last: string) -> () {
    print(\"{last}\")
}",
        "TPZ5024",
    );
    assert_clean(
        "function f(first: string, ...xs: int) -> () {
    print(\"{first}\")
}",
    );
}

#[test]
fn generic_alias_substitution_forms_concretely() {
    // W<int> must form Array<int>, not Array<?param>.
    // `>` is a trailing-continuation token (SPEC §1a), so a
    // generic-final alias needs the explicit `;` separator before a
    // following statement.
    let src = "type W<T> = Array<T>;
type U = W<int>";
    let out = topaz_parser::parse_with_options(
        topaz_diag::FileId(0),
        src,
        topaz_parser::ParseOptions {
            language_version: LangVersion::V5_2,
        },
    );
    assert!(out.diagnostics.is_empty());
    let mut former = topaz_check::Former::new(src, &out.program);
    former.validate_aliases();
    assert!(former.diagnostics.is_empty());
    // Form the use site directly: alias body of U.
    let items = &out.program.items;
    let alias = match &items[1].kind {
        topaz_syntax::ast::StmtKind::TypeAlias(a) => a,
        _ => unreachable!(),
    };
    let formed = former.form(&alias.ty, &std::collections::HashMap::new());
    assert_eq!(formed, Type::Ctor(Ctor::Array, vec![int()]));
}

#[test]
fn qualified_types_form_as_opaque_foreign() {
    // Single-file programs cannot import, but the type POSITION must
    // form without a false diagnostic and still walk its arguments.
    let src = "function f(u: ns.User) -> () {
    print(\"{u}\")
}";
    let diags = check(src);
    assert!(diags.is_empty(), "qualified type must not error: {diags:?}");
    assert_code(
        "function f(u: ns.Box<Array<int, int>>) -> () {
    print(\"{u}\")
}",
        "TPZ5022",
    );
}

// ---- subtyping -------------------------------------------------------

fn int() -> Type {
    Type::Prim(Prim::Int)
}

fn string() -> Type {
    Type::Prim(Prim::String)
}

#[test]
fn literal_widens_to_its_primitive() {
    assert!(is_subtype(&Type::Literal(Lit::Int(1)), &int()));
    assert!(is_subtype(
        &Type::Literal(Lit::Str("open".into())),
        &string()
    ));
    assert!(!is_subtype(&Type::Literal(Lit::Int(1)), &string()));
    assert!(!is_subtype(&int(), &Type::Literal(Lit::Int(1))));
}

#[test]
fn union_membership_and_subsumption() {
    let status = Type::union(vec![
        Type::Literal(Lit::Str("open".into())),
        Type::Literal(Lit::Str("closed".into())),
    ]);
    assert!(is_subtype(&Type::Literal(Lit::Str("open".into())), &status));
    // CDR-004 widening witness: a plain string does NOT satisfy the
    // literal union.
    assert!(!is_subtype(&string(), &status));
    let nullable = Type::union(vec![int(), Type::Literal(Lit::Null)]);
    assert!(is_subtype(&int(), &nullable));
    assert!(is_subtype(&status, &string()));
}

#[test]
fn collections_are_invariant() {
    let ints = Type::Ctor(Ctor::Array, vec![int()]);
    let equivalent_ints = Type::Ctor(Ctor::Array, vec![int()]);
    let mixed = Type::Ctor(Ctor::Array, vec![Type::union(vec![int(), string()])]);
    // The CDR-004 §3 counterexample: Array<int> must NOT be an
    // Array<int | string> — pushing "bad" would corrupt the alias.
    assert!(!is_subtype(&ints, &mixed));
    assert!(!is_subtype(&mixed, &ints));
    assert!(is_subtype(&ints, &equivalent_ints));
}

#[test]
fn option_and_result_are_covariant() {
    let some_int = Type::Ctor(Ctor::Option, vec![int()]);
    let some_either = Type::Ctor(Ctor::Option, vec![Type::union(vec![int(), string()])]);
    assert!(is_subtype(&some_int, &some_either));
    assert!(!is_subtype(&some_either, &some_int));
}

#[test]
fn records_are_exact_shape() {
    let full = Type::Record(vec![("x".into(), int()), ("y".into(), int())]);
    let narrow = Type::Record(vec![("x".into(), int())]);
    // No width subtyping (record equality reasons over field sets).
    assert!(!is_subtype(&full, &narrow));
    assert!(!is_subtype(&narrow, &full));
    // Depth covariance on the same field set.
    let wide_field = Type::Record(vec![("x".into(), Type::union(vec![int(), string()]))]);
    assert!(is_subtype(&narrow, &wide_field));
    assert!(!is_subtype(&wide_field, &narrow));
}

#[test]
fn functions_are_contra_co() {
    let take_either = Type::Func {
        params: vec![Type::union(vec![int(), string()])],
        variadic: None,
        ret: Box::new(Type::Literal(Lit::Int(1))),
    };
    let take_int = Type::Func {
        params: vec![int()],
        variadic: None,
        ret: Box::new(int()),
    };
    // Wider parameter + narrower return is the subtype.
    assert!(is_subtype(&take_either, &take_int));
    assert!(!is_subtype(&take_int, &take_either));
}

#[test]
fn map_set_invariance_and_result_covariance() {
    let m_int = Type::Ctor(Ctor::Map, vec![string(), int()]);
    let m_wide = Type::Ctor(
        Ctor::Map,
        vec![string(), Type::union(vec![int(), string()])],
    );
    assert!(!is_subtype(&m_int, &m_wide));
    let s_int = Type::Ctor(Ctor::Set, vec![int()]);
    let s_wide = Type::Ctor(Ctor::Set, vec![Type::union(vec![int(), string()])]);
    assert!(!is_subtype(&s_int, &s_wide));
    let r = Type::Ctor(Ctor::Result, vec![int(), string()]);
    let r_wide = Type::Ctor(
        Ctor::Result,
        vec![Type::union(vec![int(), string()]), string()],
    );
    assert!(is_subtype(&r, &r_wide));
    assert!(!is_subtype(&r_wide, &r));
}

#[test]
fn variadic_function_subtyping_is_contravariant() {
    let take_ints = Type::Func {
        params: vec![],
        variadic: Some(Box::new(int())),
        ret: Box::new(Type::Prim(Prim::Unit)),
    };
    let take_either = Type::Func {
        params: vec![],
        variadic: Some(Box::new(Type::union(vec![int(), string()]))),
        ret: Box::new(Type::Prim(Prim::Unit)),
    };
    assert!(is_subtype(&take_either, &take_ints));
    assert!(!is_subtype(&take_ints, &take_either));
    let fixed = Type::Func {
        params: vec![int()],
        variadic: None,
        ret: Box::new(Type::Prim(Prim::Unit)),
    };
    assert!(!is_subtype(&fixed, &take_ints));
}

#[test]
fn union_of_records_vs_record() {
    let a = Type::Record(vec![("x".into(), int())]);
    let b = Type::Record(vec![("y".into(), int())]);
    let u = Type::union(vec![a.clone(), b.clone()]);
    assert!(is_subtype(&a, &u));
    assert!(!is_subtype(&u, &a));
}

#[test]
fn foreign_types_are_identity_compared() {
    let x = Type::Foreign {
        name: "ns.User".into(),
        args: vec![],
    };
    let equivalent_x = Type::Foreign {
        name: "ns.User".into(),
        args: vec![],
    };
    let y = Type::Foreign {
        name: "ns.Other".into(),
        args: vec![],
    };
    assert!(is_subtype(&x, &equivalent_x));
    assert!(!is_subtype(&x, &y));
}

#[test]
fn union_normalization_is_canonical() {
    let a = Type::union(vec![int(), string()]);
    let b = Type::union(vec![string(), Type::union(vec![int(), string()])]);
    assert_eq!(a, b);
    assert_eq!(Type::union(vec![int(), int()]), int());
}
