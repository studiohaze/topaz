use super::*;

#[test]
fn return_case_arms_are_v52_only() {
    // SPEC v5.2 §5 (ADR-074): `CaseArmBody ::= Expression | ReturnStmt`.
    let src = "function f(x: int) -> int {
    match x {
        case 0 => return 1
        case _ => x
    }
}";
    let program = parse_ok_v52(src);
    let StmtKind::Function(decl) = &program.items[0].kind else {
        panic!("expected function");
    };
    let Some(tail) = &decl.body.tail else {
        panic!("expected trailing match, got {:?}", decl.body);
    };
    let ExprKind::Match { cases, .. } = &tail.kind else {
        panic!("expected match tail");
    };
    assert!(matches!(
        cases[0].body,
        CaseArmBody::Return { value: Some(_), .. }
    ));
    assert!(matches!(cases[1].body, CaseArmBody::Expr(_)));

    // The same source is not v5.1: `return` cannot begin an arm
    // expression, and the v0.1 diagnostic shape is preserved.
    assert!(!parse(FileId(0), src).diagnostics.is_empty());
}

#[test]
fn return_arm_value_is_optional_and_sep_bounded() {
    let src = "function f(x: int) -> () {
    match x {
        case 0 => return
        case _ => ()
    }
}";
    let program = parse_ok_v52(src);
    let StmtKind::Function(decl) = &program.items[0].kind else {
        panic!("expected function");
    };
    let Some(tail) = &decl.body.tail else {
        panic!("expected trailing match");
    };
    let ExprKind::Match { cases, .. } = &tail.kind else {
        panic!("expected match tail");
    };
    // Bare `return` arm: the newline separator ends the arm, so the
    // next `case` clause is not swallowed as a return value.
    assert!(matches!(
        cases[0].body,
        CaseArmBody::Return { value: None, .. }
    ));
    assert_eq!(cases.len(), 2);
}

#[test]
fn or_patterns_are_v52_only() {
    // SPEC v5.2 §6 (ADR-073): literal, range, and bindingless
    // structural alternatives.
    let src = "match x {
    case \"A\" | \"B\" => 1
    case 0..1 | 5..9 => 2
    case Ok(_) | Err(_) => 3
    case _ => 0
}";
    let program = parse_ok_v52(src);
    let StmtKind::Expr(Expr {
        kind: ExprKind::Match { cases, .. },
        ..
    }) = &program.items[0].kind
    else {
        panic!("expected match");
    };
    for case in &cases[..3] {
        let PatternKind::Or(alts) = &case.pattern.kind else {
            panic!("expected or-pattern, got {:?}", case.pattern);
        };
        assert_eq!(alts.len(), 2);
    }
    // Single-alternative patterns are never wrapped.
    assert!(matches!(cases[3].pattern.kind, PatternKind::Wildcard));

    // v5.1 rejects the same source (the `|` does not parse).
    assert!(!parse(FileId(0), src).diagnostics.is_empty());
}

#[test]
fn enum_declarations_are_v53_only() {
    // §3 (v5.3): `enum Name { … }` is a user-enum declaration ONLY at v5.3.
    let src = "enum Color { Red, Blue }\nlet c: Color = Color.Red\n";
    let program = parse_ok_v53(src);
    assert!(
        matches!(&program.items[0].kind, StmtKind::Enum(decl)
            if decl.variants.len() == 2),
        "expected an enum decl at v5.3, got {:?}",
        program.items[0].kind
    );

    // At v5.2 (and v5.1) the SAME source does NOT parse as an enum decl: `enum`
    // is an ordinary identifier, so `enum Color { … }` is a malformed statement
    // (the feature is unavailable). Both lower versions reject it.
    assert!(
        !parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::V5_2,
            },
        )
        .diagnostics
        .is_empty(),
        "enum must NOT parse at v5.2"
    );
    assert!(
        !parse(FileId(0), src).diagnostics.is_empty(),
        "enum must NOT parse at v5.1"
    );

    // `enum` stays usable as an ordinary identifier at v5.3 (contextual keyword):
    // `let enum = 5` binds a variable named `enum`.
    let id = parse_ok_v53("let enum = 5\nprint(\"{enum}\")\n");
    assert!(matches!(id.items[0].kind, StmtKind::Let { .. }));

    // A v5.2 FEATURE (import/export) still parses at v5.3 (strict superset).
    let sup = parse_ok_v53("export function ok() -> int { 1 }\n");
    assert!(matches!(&sup.items[0].kind, StmtKind::Export(_)));
}

#[test]
fn or_pattern_alternatives_must_not_bind() {
    // Direct binding, constructor binding, and record shorthand all
    // hit TPZ2006; `_` alternatives are not bindings.
    for src in [
        "match x {
    case a | b => 1
    case _ => 0
}",
        "match x {
    case Ok(v) | Err(v) => 1
    case _ => 0
}",
        "match x {
    case { name } | _ => 1
    case _ => 0
}",
    ] {
        assert!(
            codes_v52(src).contains(&"TPZ2006".to_string()),
            "expected TPZ2006 for {src:?}"
        );
    }
    parse_ok_v52(
        "match x {
    case _ | 0 => 1
    case _ => 0
}",
    );
}

#[test]
fn or_pattern_alternatives_may_bind_at_v54() {
    // §6 (v5.4) BINDING or-patterns: an alternative MAY bind names. The same
    // sources the v5.2 grammar rejects (TPZ2006) now parse with ZERO parser
    // diagnostics at v5.4 — agreement is the CHECKER's job (TPZ5710/5711), not the
    // parser's. A direct binding, a constructor binding, and a record-shorthand
    // binding all parse.
    for src in [
        "match x {
    case a | b => 1
    case _ => 0
}",
        "match x {
    case Ok(v) | Err(v) => 1
    case _ => 0
}",
        "match x {
    case { name } | { name } => 1
    case _ => 0
}",
    ] {
        let program = parse_ok_v54(src);
        let StmtKind::Expr(Expr {
            kind: ExprKind::Match { cases, .. },
            ..
        }) = &program.items[0].kind
        else {
            panic!("expected match for {src:?}");
        };
        assert!(
            matches!(cases[0].pattern.kind, PatternKind::Or(_)),
            "expected an or-pattern for {src:?}"
        );
    }

    // The v5.3 edition STILL rejects a binding alternative (TPZ2006) — the
    // restriction is lifted only at `>= V5_4`.
    let bind_src = "match x {
    case Ok(v) | Err(v) => 1
    case _ => 0
}";
    let v53 = parse_with_options(
        FileId(0),
        bind_src,
        ParseOptions {
            language_version: LangVersion::V5_3,
        },
    );
    assert!(
        v53.diagnostics.iter().any(|d| d.code.as_str() == "TPZ2006"),
        "v5.3 must still reject a binding or-pattern alternative"
    );
}

#[test]
fn type_union_binds_before_pattern_pipe() {
    // SPEC v5.2 §6: `case x: int | string =>` is one TypePattern with
    // a union type — never an or-pattern of `x: int` and `string`.
    let src = "match x {
    case n: int | string => 1
    case _ => 0
}";
    let program = parse_ok_v52(src);
    let StmtKind::Expr(Expr {
        kind: ExprKind::Match { cases, .. },
        ..
    }) = &program.items[0].kind
    else {
        panic!("expected match");
    };
    let PatternKind::Typed { ty, .. } = &cases[0].pattern.kind else {
        panic!(
            "expected TypePattern reading, got {:?}",
            cases[0].pattern.kind
        );
    };
    assert!(matches!(ty.kind, TypeKind::Union(_)));
}

#[test]
fn pattern_pipe_is_a_layout_continuation_in_pattern_regions() {
    // SPEC v5.2 §1a: pattern-level `|` continues lines in pattern
    // regions (trailing and leading forms) — the match body is
    // separator mode, so without the rule these would split items.
    parse_ok_v52(
        "match x {
    case \"A\" |
        \"B\" => 1
    case _ => 0
}",
    );
    parse_ok_v52(
        "match x {
    case \"A\"
        | \"B\" => 1
    case _ => 0
}",
    );
    // Expression-position `|`/`||` layout is unchanged at v5.2: a
    // trailing `||` still continues (v5.1 rule), a leading one does
    // not become a pattern continuation outside pattern regions.
    parse_ok_v52(
        "let ok = a ||
    b",
    );
}

#[test]
fn keyword_field_names_in_the_seven_positions() {
    // SPEC v5.2 §8 (ADR-075): record literal, record constant,
    // record update, record type, explicit record-pattern field,
    // member access, optional member access/call.
    parse_ok_v52("let r = { type: \"email\" }");
    parse_ok_v52("const C = { type: 1 }");
    parse_ok_v52(
        "let r = { type: 1 }
let u = r { type: 2 }",
    );
    parse_ok_v52("let r: { type: string } = make()");
    parse_ok_v52(
        "match r {
    case { type: t } => t
    case _ => 0
}",
    );
    parse_ok_v52("let k = task.type");
    parse_ok_v52("let k = task?.type");
    parse_ok_v52("let k = task?.type()");

    // A keyword member remains a field name for layout as well as
    // parsing; it must not turn the following construct body into a
    // continuation-mode record-pattern brace.
    for access in [".", "?."] {
        for field in ["let", "const", "case", "for", "concurrent"] {
            parse_ok_v52(&format!(
                "if task{access}{field} {{\n    one()\n    two()\n}}"
            ));
        }
    }

    // Keyword record fields likewise do not create binding/concurrent
    // layout state. The nested explicit record pattern keeps its `|`
    // continuation across the physical newline.
    parse_ok_v52(
        "let {
    let: outer,
    concurrent: {
        y: Some(_)
            | None,
    },
} = value",
    );

    // Statement-position `{ type: 1 }` is a record literal at v5.2
    // (layout + parser lookaheads agree on FieldName).
    parse_ok_v52("{ type: 1 }");

    // All of it stays invalid v5.1 (keyword in identifier position).
    for src in [
        "let r = { type: \"email\" }",
        "let k = task.type",
        "let k = task?.type",
    ] {
        assert!(
            !parse(FileId(0), src).diagnostics.is_empty(),
            "expected v5.1 rejection for {src:?}"
        );
    }
}

#[test]
fn keyword_field_names_rejected_outside_the_seven_positions() {
    // SPEC v5.2 §8 rejected positions (parser-owned ones).
    // Named-argument label:
    assert!(!codes_v52("f(type: x)").is_empty());
    // Record-pattern shorthand (would bind a keyword):
    assert!(
        !codes_v52(
            "match r {
    case { type } => 1
    case _ => 0
}"
        )
        .is_empty()
    );
    // `concurrent` arm names stay Identifier-only:
    assert!(
        !codes_v52(
            "let x = concurrent {
    type: fetch()
}"
        )
        .is_empty()
    );
    // Binding names:
    assert!(!codes_v52("let type = 1").is_empty());
    // Pipe field sugar stays identifier-only (SPEC §11 — not one of
    // the seven positions):
    assert!(!codes_v52("let k = task |> .type").is_empty());
    // Construct braces are classified by their construct, never as
    // records (ADR-075 required negatives):
    assert!(!codes_v52("if ok { type: \"email\" }").is_empty());
    assert!(!codes_v52("match x { case: 1 }").is_empty());
}

#[test]
fn module_items_parse_at_v52() {
    // SPEC v5.2 §17: Form A, alias, Form B with per-name aliases.
    let program = parse_ok_v52(
        "import utils.strings\nimport net.http as web\nimport data.csv { parse, headers as heads }\nlet x = 1",
    );
    let StmtKind::Import(item) = &program.items[0].kind else {
        panic!("expected import");
    };
    assert_eq!(item.path.segments.len(), 2);
    assert!(matches!(item.kind, ImportKind::Namespace { alias: None }));
    let StmtKind::Import(item) = &program.items[1].kind else {
        panic!("expected import");
    };
    assert!(matches!(
        item.kind,
        ImportKind::Namespace { alias: Some(_) }
    ));
    let StmtKind::Import(item) = &program.items[2].kind else {
        panic!("expected import");
    };
    let ImportKind::Selected { specs } = &item.kind else {
        panic!("expected selection");
    };
    assert_eq!(specs.len(), 2);
    assert!(specs[0].alias.is_none());
    assert!(specs[1].alias.is_some());

    // Multi-line import list: the brace is continuation mode
    // (SPEC v5.2 §1a), so inner newlines do not split items.
    parse_ok_v52("import data.csv {\n    parse,\n    headers as heads,\n}\nlet x = 1");
}

#[test]
fn import_selection_duplicates_report_once_in_source_order() {
    let source = "import tools { first as x, second as y, third as y, fourth as x, fifth as x }\n";
    let diagnostics = parse_with_options(
        FileId(0),
        source,
        ParseOptions {
            language_version: LangVersion::V5_2,
        },
    )
    .diagnostics;
    let observed = diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str(),
                diagnostic.message.as_str(),
                diagnostic.primary.span.lo,
                diagnostic.primary.span.hi,
            )
        })
        .collect::<Vec<_>>();
    const DUPLICATE_LOCAL: &str = "this local name is already bound by this list";
    assert_eq!(
        observed,
        vec![
            ("TPZ2011", DUPLICATE_LOCAL, 49, 50),
            ("TPZ2011", DUPLICATE_LOCAL, 62, 63),
            ("TPZ2011", DUPLICATE_LOCAL, 74, 75),
        ]
    );
}

#[test]
fn export_items_parse_at_v52() {
    let program = parse_ok_v52(
        "export function f(x: int) -> int { x }\nexport type User = { id: int }\nexport let limit = 10\nexport const MAX = 99",
    );
    for item in &program.items {
        assert!(matches!(item.kind, StmtKind::Export(_)), "{item:?}");
    }
    // `export let mut` parses (rejection is resolver-era).
    parse_ok_v52("export let mut state = 0");
}

#[test]
fn export_nominal_items_parse_at_v54() {
    let program = parse_ok_v54(
        "export enum Msg { Noop, Inc(int) }\n\
         export record User { name: string }\n\
         export newtype UserId = int",
    );
    assert_eq!(program.items.len(), 3);
    assert!(matches!(
        &program.items[0].kind,
        StmtKind::Export(inner) if matches!(inner.kind, StmtKind::Enum(_))
    ));
    assert!(matches!(
        &program.items[1].kind,
        StmtKind::Export(inner) if matches!(inner.kind, StmtKind::Record(_))
    ));
    assert!(matches!(
        &program.items[2].kind,
        StmtKind::Export(inner) if matches!(inner.kind, StmtKind::Newtype(_))
    ));
}

#[test]
fn generic_nominal_items_parse_at_v54() {
    let program = parse_ok_v54(
        "enum Maybe<T> { None, Some(T) }\n\
         record Box<T> { value: T }\n\
         newtype Id<T> = T",
    );
    let StmtKind::Enum(decl) = &program.items[0].kind else {
        panic!("expected enum");
    };
    assert_eq!(decl.type_params.len(), 1);
    let StmtKind::Record(decl) = &program.items[1].kind else {
        panic!("expected record");
    };
    assert_eq!(decl.type_params.len(), 1);
    let StmtKind::Newtype(decl) = &program.items[2].kind else {
        panic!("expected newtype");
    };
    assert_eq!(decl.type_params.len(), 1);
}

#[test]
fn module_envelope_base_priority() {
    // Head words with non-allow-listed follows keep their base
    // reading at v5.2 (ADR-076 base priority).
    parse_ok_v52("import");
    parse_ok_v52("let a = export");
    parse_ok_v52("import(x)");
    parse_ok_v52("import = 5");
    parse_ok_v52("export\nfunction f() -> () { () }");
    parse_ok_v52("record.use");
    parse_ok_v52("let use = 1");
    // And the same sources stay v5.1-identical (C3-style pins).
    parse_ok("import");
    parse_ok("import(x)");
    parse_ok("export\nfunction f() -> () { () }");
}

#[test]
fn module_adjacent_forms_get_module_diagnostics() {
    // Rejected module-adjacent forms (TPZ2009).
    assert!(codes_v52("export { a, b }").contains(&"TPZ2009".to_string()));
    assert!(codes_v52("export import utils.strings").contains(&"TPZ2009".to_string()));
    assert!(codes_v52("import utils.strings as ns { trim }").contains(&"TPZ2009".to_string()));
    // Reserved-unused forms (TPZ2008).
    assert!(codes_v52("use math").contains(&"TPZ2008".to_string()));
    assert!(codes_v52("import \"x\"").contains(&"TPZ2008".to_string()));
    // Prologue violation (TPZ2010).
    assert!(codes_v52("let x = 1\nimport utils.strings").contains(&"TPZ2010".to_string()));
    // Exported destructuring (TPZ2007).
    assert!(codes_v52("export let { a } = v").contains(&"TPZ2007".to_string()));
    // Tagged-template adjacency keeps its base diagnostic (unknown
    // template tag), never a module diagnostic.
    let codes = codes_v52("import\"x\"");
    assert!(codes.contains(&"TPZ2002".to_string()), "{codes:?}");
    // All module items stay invalid at v5.1.
    assert!(
        !parse(FileId(0), "import utils.strings")
            .diagnostics
            .is_empty()
    );
    assert!(
        !parse(FileId(0), "export function f() -> () { () }")
            .diagnostics
            .is_empty()
    );
}

#[test]
fn qualified_named_types_are_v52_only() {
    // SPEC v5.2 §3: `QualifiedNamedType ::= Identifier "." Identifier
    // TypeArgs?` joins PrimaryType at v5.2 only.
    let program = parse_ok_v52(
        "import users
let u: users.User = make()
let l: users.List<int> = make()",
    );
    // `let u: T` routes through the typed pattern (v0.1 shape); the
    // qualified type sits in the pattern's type slot.
    let StmtKind::Let { pattern, .. } = &program.items[1].kind else {
        panic!("expected let");
    };
    let PatternKind::Typed { ty, .. } = &pattern.kind else {
        panic!("expected typed pattern, got {:?}", pattern.kind);
    };
    assert!(matches!(ty.kind, TypeKind::Qualified { .. }));
    // v5.1 type position has no dot.
    assert!(
        !parse(FileId(0), "let u: users.User = make()")
            .diagnostics
            .is_empty()
    );
}

#[test]
fn none_is_never_a_binding_name() {
    // SPEC §6/§22.1: bare `None` in pattern position is a zero-arg
    // constructor pattern, and no binding position may take the name.
    let program = parse_ok_v52("match x {\n    case None => 0\n    case _ => 1\n}");
    let StmtKind::Expr(expr) = &program.items[0].kind else {
        panic!("expected expr");
    };
    let ExprKind::Match { cases, .. } = &expr.kind else {
        panic!("expected match");
    };
    let PatternKind::Constructor { args, .. } = &cases[0].pattern.kind else {
        panic!(
            "expected constructor pattern, got {:?}",
            cases[0].pattern.kind
        );
    };
    assert!(args.is_empty());

    for src in [
        "let mut None = 1",
        "const None: Option<int> = None",
        "function inspect(None: int) -> int { 1 }",
        "let inspect: (int) -> int = None => 1",
        "let inspect = (None: int) => 1",
        "let None: Option<int> = make()",
        "match x {\n    case None: Option<int> => 0\n    case _ => 1\n}",
        "let { None } = make()",
    ] {
        assert!(
            codes_v52(src).contains(&"TPZ2012".to_string()),
            "expected TPZ2012 for {src:?}"
        );
    }
    // Explicit-field record patterns may still MATCH a field named
    // None against a non-binding pattern.
    parse_ok_v52("match x {\n    case { None: _ } => 0\n    case _ => 1\n}");
}

#[test]
fn reserves_tilde_as_no_bitwise() {
    // `~` is reserved (TPZ2013): Topaz is arithmetic-only, no bitwise operators.
    for src in ["let x = ~5", "let y = ~~1", "let z = -~2"] {
        assert!(
            codes_v52(src).contains(&"TPZ2013".to_string()),
            "expected TPZ2013 for {src:?}"
        );
    }
}

#[test]
fn err_003_generic_final_lines_separate() {
    // ERR-003: `>`/`>>` are leading-continuation only, so
    // consecutive generic-final statements need no `;`.
    let program = parse_ok_v52(
        "type W<T> = Array<T>
type M = Array<Array<int>>
let xs: W<int> = []
let m: M = []",
    );
    assert_eq!(program.items.len(), 4);

    // Leading `>` still continues a comparison.
    let program = parse_ok_v52("let a = 1\n    > 0\nprint(\"{a}\")");
    assert_eq!(program.items.len(), 2);

    // A trailing `>` no longer absorbs the next line.
    assert!(
        !parse_with_options(
            FileId(0),
            "let a = 1 >\n2",
            ParseOptions {
                language_version: LangVersion::V5_2,
            },
        )
        .diagnostics
        .is_empty()
    );
}

#[test]
fn err_003_is_v52_only() {
    // The frozen v5.1 keeps trailing `>`/`>>` continuation: the
    // comparison still absorbs the next line.
    let out = parse(FileId(0), "let a = 1 >\n2");
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(out.program.items.len(), 1);
}
