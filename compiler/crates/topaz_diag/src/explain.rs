//! Stable diagnostic explanation registry.
//!
//! The registry is intentionally static and dependency-free: agents can call
//! `topaz explain TPZ#### --json` without loading source files or reproducing a
//! diagnostic first.

use std::fmt::Write as _;

use crate::Code;

/// Concrete bad/good examples for one diagnostic explanation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplainExamples {
    pub bad: &'static str,
    pub good: &'static str,
}

/// A stable diagnostic-code explanation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticExplanation {
    pub code: &'static str,
    pub phase: &'static str,
    pub machine_kind: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub why: &'static str,
    pub examples: Option<ExplainExamples>,
    pub fixits: &'static [&'static str],
}

/// Looks up a stable diagnostic explanation by code.
pub fn explain_code(code: &str) -> Option<&'static DiagnosticExplanation> {
    EXPLANATIONS.iter().find(|entry| entry.code == code)
}

/// True when `code` has the public `TPZ` + four digit shape.
pub fn is_explain_code_shape(code: &str) -> bool {
    Code::has_registry_shape(code)
}

/// Renders an explanation for humans.
pub fn render_explain(explanation: &DiagnosticExplanation) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}: {}", explanation.code, explanation.title);
    let _ = writeln!(out, "phase: {}", explanation.phase);
    let _ = writeln!(out, "machine.kind: {}", explanation.machine_kind);
    out.push('\n');
    let _ = writeln!(out, "{}", explanation.summary);
    out.push('\n');
    let _ = writeln!(out, "why: {}", explanation.why);
    if let Some(examples) = explanation.examples {
        out.push('\n');
        out.push_str("bad:\n");
        indent_block(&mut out, examples.bad);
        out.push_str("good:\n");
        indent_block(&mut out, examples.good);
    }
    if !explanation.fixits.is_empty() {
        out.push('\n');
        out.push_str("fix-it:\n");
        for fixit in explanation.fixits {
            let _ = writeln!(out, "  - {fixit}");
        }
    }
    out
}

/// Renders an explanation as one deterministic JSON object.
pub fn render_explain_json(explanation: &DiagnosticExplanation) -> String {
    let mut s = String::from("{\"code\":");
    push_json_string(&mut s, explanation.code);
    s.push_str(",\"phase\":");
    push_json_string(&mut s, explanation.phase);
    s.push_str(",\"machine\":{\"kind\":");
    push_json_string(&mut s, explanation.machine_kind);
    s.push_str("},\"title\":");
    push_json_string(&mut s, explanation.title);
    s.push_str(",\"summary\":");
    push_json_string(&mut s, explanation.summary);
    s.push_str(",\"why\":");
    push_json_string(&mut s, explanation.why);
    s.push_str(",\"examples\":");
    if let Some(examples) = explanation.examples {
        s.push_str("{\"bad\":");
        push_json_string(&mut s, examples.bad);
        s.push_str(",\"good\":");
        push_json_string(&mut s, examples.good);
        s.push('}');
    } else {
        s.push_str("null");
    }
    s.push_str(",\"fixits\":[");
    for (i, fixit) in explanation.fixits.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        push_json_string(&mut s, fixit);
    }
    s.push_str("]}");
    s
}

fn indent_block(out: &mut String, raw: &str) {
    for line in raw.lines() {
        let _ = writeln!(out, "  {line}");
    }
}

fn push_json_string(s: &mut String, raw: &str) {
    s.push('"');
    for c in raw.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(s, "\\u{:04x}", c as u32);
            }
            c => s.push(c),
        }
    }
    s.push('"');
}

macro_rules! explanation {
    ($code:literal, $phase:literal, $machine_kind:literal, $title:literal, $summary:literal, $why:literal, $fixit:literal $(,)?) => {
        DiagnosticExplanation {
            code: $code,
            phase: $phase,
            machine_kind: $machine_kind,
            title: $title,
            summary: $summary,
            why: $why,
            examples: None,
            fixits: &[$fixit],
        }
    };
}

static EXPLANATIONS: &[DiagnosticExplanation] = &[
    explanation!(
        "TPZ0001",
        "lex",
        "unknown_character",
        "Unknown character",
        "The lexer found a source character that is not part of the Topaz token set.",
        "Lexical errors are reported before layout or parsing so recovery starts at a known byte boundary.",
        "remove or replace the unsupported character",
    ),
    explanation!(
        "TPZ0002",
        "lex",
        "unterminated_block_comment",
        "Unterminated block comment",
        "A block comment reached the end of input before its closing delimiter.",
        "Comments are skipped by the lexer, so unterminated comments must be diagnosed before parsing.",
        "add the closing block-comment delimiter",
    ),
    explanation!(
        "TPZ0003",
        "lex",
        "unterminated_string",
        "Unterminated string",
        "A string or template literal ended before its closing delimiter.",
        "String tokenization owns escaping and interpolation boundaries; later phases cannot infer the intended end.",
        "close the string/template literal or escape the delimiter",
    ),
    explanation!(
        "TPZ0004",
        "lex",
        "invalid_escape",
        "Invalid escape sequence",
        "A string or template literal contains an escape sequence Topaz does not define.",
        "Escapes are normalized by the lexer so runtime string values are deterministic.",
        "replace the escape with a supported spelling or a literal character",
    ),
    explanation!(
        "TPZ0005",
        "lex",
        "stray_brace_in_string",
        "Stray brace in string",
        "A template/text string contains a brace that is not a valid interpolation boundary.",
        "Template interpolation uses explicit braces and must stay balanced in the lexer.",
        "escape the brace or complete the interpolation expression",
    ),
    explanation!(
        "TPZ0006",
        "lex",
        "template_indent",
        "Template indentation mismatch",
        "A multiline template line does not match the closing delimiter indentation rule.",
        "Indent normalization is lexical so every target sees the same template bytes.",
        "align the template content with the closing delimiter indentation",
    ),
    explanation!(
        "TPZ1001",
        "layout",
        "semicolon_in_delimiter_list",
        "Semicolon in delimiter list",
        "A semicolon appeared where a comma/newline-separated delimiter list is required.",
        "The layout pass keeps delimiter lists unambiguous before parsing.",
        "replace the semicolon with a comma or split the construct into statements",
    ),
    DiagnosticExplanation {
        code: "TPZ2001",
        phase: "parse",
        machine_kind: "unexpected_token",
        title: "Unexpected token",
        summary: "The parser found a token that cannot appear in this grammar position.",
        why: "Topaz keeps parse recovery local so later diagnostics stay anchored to the real source shape.",
        examples: None,
        fixits: &["rewrite the construct using the canonical grammar for that position"],
    },
    DiagnosticExplanation {
        code: "TPZ2002",
        phase: "parse",
        machine_kind: "unknown_template_tag",
        title: "Unknown template tag",
        summary: "A tagged template used a tag that is not in the built-in template registry.",
        why: "Only registry tags with fixed semantics are accepted; user-defined template tags are deferred.",
        examples: Some(ExplainExamples {
            bad: "html\"<p>{name}</p>\"",
            good: "sql\"\"\"select * from users where id = {id}\"\"\"",
        }),
        fixits: &["use a registered template tag or a normal string/template value"],
    },
    explanation!(
        "TPZ2003",
        "parse",
        "invalid_assignment_target",
        "Invalid assignment target",
        "The left side of an assignment is not an assignable place.",
        "Assignments can only target bindings, member paths, or index paths with a mutable root.",
        "assign to an identifier/member/index path or rewrite as a value expression",
    ),
    explanation!(
        "TPZ2004",
        "parse",
        "invalid_defer_body",
        "Invalid defer body",
        "A defer statement body is neither a block nor a call expression.",
        "Defer bodies must have explicit effect shape so exit-time execution stays predictable.",
        "wrap the deferred work in a block or call a function directly",
    ),
    explanation!(
        "TPZ2005",
        "parse",
        "concurrent_form",
        "Malformed concurrent form",
        "A concurrent timeout and else branch are not paired correctly.",
        "Timeout semantics require an explicit else branch, and else belongs only to timeout concurrency.",
        "add the missing timeout/else pair or remove the unmatched branch",
    ),
    explanation!(
        "TPZ2006",
        "parse",
        "or_pattern_binding",
        "Binding or-pattern is not available in this language mode",
        "An or-pattern alternative binds a name where that mode requires non-binding alternatives.",
        "Older language modes keep or-pattern alternatives binding-free; v5.4 checks binding agreement separately.",
        "switch to v5.4 or rewrite the pattern without bound alternatives",
    ),
    explanation!(
        "TPZ2007",
        "parse",
        "export_binding_form",
        "Invalid exported let pattern",
        "An exported let declaration uses a pattern other than one identifier.",
        "Module exports publish named values; destructuring exports would create an unclear public surface.",
        "bind one exported identifier, then destructure internally if needed",
    ),
    explanation!(
        "TPZ2008",
        "parse",
        "reserved_module_form",
        "Reserved module form",
        "The source uses a module syntax form reserved but not defined by the current grammar.",
        "Reserved forms diagnose early so future module features cannot be accidentally inferred today.",
        "use the two canonical import forms and inline export declarations",
    ),
    explanation!(
        "TPZ2009",
        "parse",
        "rejected_module_form",
        "Rejected module-adjacent form",
        "The source uses a module export/import composition that Topaz does not support.",
        "v5.2+ keeps the module surface small: imports are declarations, exports are inline declarations.",
        "rewrite using canonical imports and inline `export`",
    ),
    explanation!(
        "TPZ2010",
        "parse",
        "import_prologue",
        "Import after non-import item",
        "An import appears after a top-level item that ends the import prologue.",
        "Imports are hoisted as a deterministic prologue before module body checking.",
        "move all imports before other top-level items",
    ),
    explanation!(
        "TPZ2011",
        "parse",
        "import_list_form",
        "Malformed import list",
        "An import selection list is empty, duplicate, or contains an invalid name.",
        "Selected imports bind a stable local namespace and cannot carry ambiguous entries.",
        "remove duplicates and use identifier-only selected imports",
    ),
    explanation!(
        "TPZ2012",
        "parse",
        "reserved_binding_name",
        "Reserved binding name",
        "A binding position uses a name reserved for a constructor or language form.",
        "Names such as `None` must stay constructor values/patterns rather than ordinary locals.",
        "choose a different local binding name",
    ),
    DiagnosticExplanation {
        code: "TPZ2013",
        phase: "parse",
        machine_kind: "reserved_operator",
        title: "Reserved operator",
        summary: "The source used an operator spelling reserved away from the current language.",
        why: "Reserved tokens fail early so agents do not infer a half-open semantic feature.",
        examples: Some(ExplainExamples {
            bad: "~flags",
            good: "!flags",
        }),
        fixits: &["replace the reserved spelling with a supported operator or helper"],
    },
    DiagnosticExplanation {
        code: "TPZ3001",
        phase: "resolve",
        machine_kind: "unresolved_module",
        title: "Unresolved module",
        summary: "An import path could not be resolved under the active module root.",
        why: "Module resolution is root-contained and deterministic; imports cannot escape to ambient paths.",
        examples: None,
        fixits: &["check the module path and the active --root directory"],
    },
    explanation!(
        "TPZ3002",
        "resolve",
        "root_containment",
        "Entry is outside module root",
        "The selected entry file is not contained by the active module root.",
        "Module resolution is root-contained so imports cannot depend on ambient filesystem location.",
        "choose a --root that contains the entry or move the entry under the root",
    ),
    explanation!(
        "TPZ3003",
        "resolve",
        "source_bound",
        "Module source is too large",
        "A module source exceeds the compiler's bounded source-map size.",
        "The bound keeps spans and source-map offsets representable and deterministic.",
        "split the module or reduce the source size",
    ),
    explanation!(
        "TPZ3004",
        "resolve",
        "module_collision",
        "Module path collision",
        "Two physical module candidates collide under exact, normalized, or folded path identity.",
        "Topaz forbids case/Unicode-ambiguous module identity so resolution is portable.",
        "rename one module path to a distinct scalar spelling",
    ),
    explanation!(
        "TPZ3005",
        "resolve",
        "physical_containment",
        "Physical path escapes root",
        "A module resolves through a physical path or symlink outside the module root.",
        "Root containment is enforced on canonical physical paths, not just textual imports.",
        "keep imports and symlinks inside the declared root",
    ),
    explanation!(
        "TPZ3006",
        "resolve",
        "import_cycle",
        "Import cycle",
        "The module graph contains a cyclic import dependency.",
        "Topaz initializes modules deterministically and rejects cyclic module initialization.",
        "break the cycle by moving shared declarations to an acyclic module",
    ),
    explanation!(
        "TPZ3007",
        "resolve",
        "imported_free_statement",
        "Runtime statement in imported module",
        "An imported module has a top-level runtime-bearing free statement.",
        "Imported modules define declarations; entry modules own top-level execution.",
        "move executable work into an exported function or make this file the entry",
    ),
    explanation!(
        "TPZ3008",
        "resolve",
        "name_collision",
        "Module namespace collision",
        "Two declarations in one module bind the same namespace name.",
        "A module has one lexical namespace so imports, types, and values cannot collide ambiguously.",
        "rename one declaration or selected import",
    ),
    DiagnosticExplanation {
        code: "TPZ3009",
        phase: "resolve",
        machine_kind: "not_exported",
        title: "Name is not exported",
        summary: "A selected import or namespace member names something the module does not export.",
        why: "Topaz modules publish an explicit surface, so private declarations are not importable.",
        examples: Some(ExplainExamples {
            bad: "import lib { hidden }",
            good: "import lib { publicName }",
        }),
        fixits: &["import an exported name or add an inline export at the producer"],
    },
    explanation!(
        "TPZ3010",
        "resolve",
        "zero_export_import",
        "Imported module exports nothing",
        "A module import targets a module with no exported surface.",
        "Topaz does not support side-effect-only imports; imported modules must publish declarations.",
        "export a value/type or remove the import",
    ),
    explanation!(
        "TPZ3011",
        "resolve",
        "export_let_mut",
        "Mutable export is rejected",
        "A module tries to export a mutable let binding.",
        "Exports are immutable views so importers cannot mutate producer state through module boundaries.",
        "export an immutable binding or expose mutation through an explicit function",
    ),
    explanation!(
        "TPZ3012",
        "resolve",
        "namespace_not_value",
        "Namespace used as value",
        "A module namespace binding appears where a runtime value is required.",
        "Namespaces qualify exported members; they are not first-class runtime values.",
        "select a concrete exported member from the namespace",
    ),
    explanation!(
        "TPZ3013",
        "resolve",
        "namespace_member_kind",
        "Namespace member has wrong kind",
        "A namespace member is used in a position that requires a different export kind.",
        "Value and type exports occupy distinct use positions after resolution.",
        "use a value export in expression position or a type export in type position",
    ),
    explanation!(
        "TPZ3014",
        "resolve",
        "private_type_in_export",
        "Private type in exported surface",
        "An exported signature mentions a type alias that the module does not export.",
        "Public module surfaces must be checkable by importers without private type knowledge.",
        "export the type alias or remove it from the exported signature",
    ),
    explanation!(
        "TPZ3015",
        "resolve",
        "readonly_import",
        "Assignment to imported binding",
        "The program attempts to assign to a binding imported from another module.",
        "Imports grant read access only; mutation across module boundaries must be explicit.",
        "assign to a local mutable value or expose an explicit producer function",
    ),
    explanation!(
        "TPZ3016",
        "resolve",
        "reserved_root",
        "Reserved module root",
        "An import addresses a root reserved by the language or standard library.",
        "Reserved roots such as `std` and `topaz` cannot be shadowed by user files.",
        "choose a non-reserved top-level module name",
    ),
    explanation!(
        "TPZ3017",
        "resolve",
        "duplicate_import",
        "Duplicate import",
        "The same logical module is imported more than once in one module.",
        "At most one import item per module keeps namespace binding and diagnostics deterministic.",
        "merge the selected names into one import item or remove the duplicate import",
    ),
    explanation!(
        "TPZ3018",
        "resolve",
        "init_forward_reference",
        "Initializer reads a later binding",
        "An imported-module initializer directly reads a later same-module runtime binding.",
        "Module initialization is eager and ordered; delayed positions may refer forward, immediate initializers may not.",
        "move the referenced declaration earlier or delay the read inside a function/lambda",
    ),
    explanation!(
        "TPZ4001",
        "runtime",
        "index_out_of_bounds",
        "Index out of bounds",
        "A runtime index operation addressed a position outside the collection/string bounds.",
        "Indexing faults rather than clamps so invalid program states do not become silent data changes.",
        "check the length first or use an Option-returning helper when available",
    ),
    explanation!(
        "TPZ4002",
        "runtime",
        "division_by_zero",
        "Division or remainder by zero",
        "An integer division or remainder operation used zero as the divisor.",
        "Arithmetic faults are shared by interpreter and emit paths so numeric behavior stays identical.",
        "guard the divisor or handle the zero case before dividing",
    ),
    explanation!(
        "TPZ4003",
        "runtime",
        "zero_range_step",
        "Range step is zero",
        "A range value used zero as its step.",
        "Ranges must make progress; a zero step would make iteration and membership undefined.",
        "use a positive or negative non-zero step",
    ),
    explanation!(
        "TPZ4004",
        "runtime",
        "integer_overflow",
        "Integer overflow",
        "A checked integer operation exceeded the `int` range.",
        "Topaz integer arithmetic faults instead of wrapping so native and interpreter results cannot drift.",
        "use a wider explicit value type or guard the operation",
    ),
    explanation!(
        "TPZ4005",
        "runtime",
        "negative_exponent",
        "Negative integer exponent",
        "Integer exponentiation received a negative exponent.",
        "Integer exponentiation stays in the integer domain and does not silently switch to floats.",
        "use a non-negative exponent or an explicit decimal/float helper",
    ),
    explanation!(
        "TPZ4006",
        "runtime",
        "match_miss",
        "Match missed at runtime",
        "A runtime match expression reached no matching arm.",
        "Unchecked or dynamic paths can still miss; checked exhaustive matches prevent this statically where possible.",
        "add a covering case or wildcard arm",
    ),
    explanation!(
        "TPZ4007",
        "runtime",
        "assertion_failed",
        "Assertion failed",
        "A std.test assertion evaluated to failure.",
        "Assertions are explicit runtime test faults, not parser-level `assert` syntax.",
        "fix the condition or expected value in the test",
    ),
    explanation!(
        "TPZ4601",
        "runtime",
        "duplicate_runtime_map_key",
        "Duplicate runtime map key",
        "A map literal or comprehension produced the same key more than once at runtime.",
        "Constant duplicate keys are static TPZ5602; value-dependent duplicates fault when evaluated.",
        "deduplicate keys before constructing the map",
    ),
    DiagnosticExplanation {
        code: "TPZ5001",
        phase: "check/runtime",
        machine_kind: "type_mismatch",
        title: "Type mismatch",
        summary: "A value's type does not match the type required by its context.",
        why: "The checker and runtime guards share this code so checked and unchecked paths report the same class of mistake.",
        examples: Some(ExplainExamples {
            bad: "let n: int = \"Ada\"",
            good: "let n: int = 42",
        }),
        fixits: &["change the value, annotation, or surrounding context so the types agree"],
    },
    DiagnosticExplanation {
        code: "TPZ5002",
        phase: "check/runtime",
        machine_kind: "unbound_name",
        title: "Unbound name",
        summary: "The program referenced a value name that is not visible in this scope.",
        why: "Topaz keeps lexical scope explicit and does not resolve through ambient globals.",
        examples: Some(ExplainExamples {
            bad: "print(answer)",
            good: "let answer = \"42\"\nprint(answer)",
        }),
        fixits: &["declare the name before use or import it from a module that exports it"],
    },
    DiagnosticExplanation {
        code: "TPZ5003",
        phase: "check/runtime",
        machine_kind: "immutable_assignment",
        title: "Immutable binding mutation",
        summary: "The program tried to assign through a binding that was not declared mutable.",
        why: "Mutation is explicit in Topaz; `let` bindings and import/namespace roots are read-only.",
        examples: Some(ExplainExamples {
            bad: "let xs = [1]\nxs.push(2)",
            good: "let mut xs = [1]\nxs.push(2)",
        }),
        fixits: &["declare the local root with `let mut` when mutation is intended"],
    },
    DiagnosticExplanation {
        code: "TPZ5004",
        phase: "check/runtime",
        machine_kind: "arity",
        title: "Wrong argument count",
        summary: "A call supplied too few, too many, or duplicate arguments for the callable shape.",
        why: "Defaults, named arguments, and variadics are normalized through one callable metadata model.",
        examples: None,
        fixits: &["match the callable signature or use the declared parameter names"],
    },
    DiagnosticExplanation {
        code: "TPZ5005",
        phase: "check/runtime",
        machine_kind: "not_callable",
        title: "Value is not callable",
        summary: "A call expression targeted a value that is not a function, method, constructor, or callable builtin.",
        why: "Topaz does not auto-convert records or data values into functions.",
        examples: Some(ExplainExamples {
            bad: "let x = 1\nx()",
            good: "function x() -> int { 1 }\nx()",
        }),
        fixits: &["call a callable value, or remove the call parentheses"],
    },
    DiagnosticExplanation {
        code: "TPZ5006",
        phase: "check/runtime",
        machine_kind: "unknown_member",
        title: "Unknown member or field",
        summary: "A member access names a field or method that does not exist on the receiver type.",
        why: "The checker rejects closed receiver shapes while gradual/unknown receivers can defer.",
        examples: Some(ExplainExamples {
            bad: "let s = \"Ada\"\ns.lenght()",
            good: "let s = \"Ada\"\ns.byteLength()",
        }),
        fixits: &["use the suggested member when present, or add the field/method to the type"],
    },
    DiagnosticExplanation {
        code: "TPZ5007",
        phase: "check/runtime",
        machine_kind: "not_comparable",
        title: "Value is not comparable or keyable",
        summary: "The operation requires equality/order/key semantics that this type does not provide.",
        why: "Map and Set keys, ordering, and membership use deterministic comparable snapshots only.",
        examples: None,
        fixits: &["derive or implement the required protocol, or project to a comparable key"],
    },
    DiagnosticExplanation {
        code: "TPZ5008",
        phase: "check/runtime",
        machine_kind: "redeclaration",
        title: "Redeclaration",
        summary: "A scope declares the same value name more than once.",
        why: "Same-scope uniqueness keeps forward references, exports, and diagnostics deterministic.",
        examples: Some(ExplainExamples {
            bad: "let x = 1\nlet x = 2",
            good: "let x = 1\nlet y = 2",
        }),
        fixits: &["rename one binding or move it into a nested scope"],
    },
    DiagnosticExplanation {
        code: "TPZ5009",
        phase: "runtime",
        machine_kind: "recursion_limit",
        title: "Recursion limit exceeded",
        summary: "Nested Topaz calls exceeded the shared call-depth limit.",
        why: "The interpreter and native backend share this guard so deep recursion cannot diverge by target.",
        examples: None,
        fixits: &["convert the recursion to a loop or reduce the recursion depth"],
    },
    DiagnosticExplanation {
        code: "TPZ5020",
        phase: "check",
        machine_kind: "unsolved_type",
        title: "Type could not be inferred",
        summary: "Inference reached a value whose type stays ambiguous without more context.",
        why: "Topaz avoids statement-order-dependent inference; empty literals and generic constructors need a context.",
        examples: Some(ExplainExamples {
            bad: "let xs = []",
            good: "let xs: Array<int> = []",
        }),
        fixits: &["add a type annotation or pass the value to a context that fixes the type"],
    },
    DiagnosticExplanation {
        code: "TPZ5021",
        phase: "check",
        machine_kind: "non_exhaustive_match",
        title: "Non-exhaustive match",
        summary: "A match expression does not cover every known case of the scrutinee type.",
        why: "Closed enums, Option, Result, booleans, and literal unions must be handled explicitly or by wildcard.",
        examples: None,
        fixits: &["add the missing case arm or an explicit wildcard arm"],
    },
    DiagnosticExplanation {
        code: "TPZ5022",
        phase: "check",
        machine_kind: "malformed_type",
        title: "Malformed or unknown type",
        summary: "A type annotation, alias, or protocol bound names a type/protocol form that is not valid here.",
        why: "Type formation is checked before value typing so downstream diagnostics do not trust malformed shapes.",
        examples: None,
        fixits: &["correct the type/protocol name or use a canonical type form"],
    },
    explanation!(
        "TPZ5023",
        "check",
        "alias_cycle",
        "Type alias cycle",
        "A type alias expands back to itself through one or more aliases.",
        "Recursive type aliases are deferred; the checker must be able to normalize aliases finitely.",
        "break the alias cycle with a nominal record/enum/newtype boundary",
    ),
    explanation!(
        "TPZ5024",
        "check",
        "variadic_position",
        "Variadic parameter is not final",
        "A variadic function type or parameter list puts the variadic slot before another slot.",
        "Variadic calls have one tail region; non-final variadics make binding ambiguous.",
        "move the variadic parameter to the final position",
    ),
    explanation!(
        "TPZ5025",
        "check",
        "invalid_qualified_type",
        "Invalid qualified type",
        "A qualified type path does not resolve through a valid namespace/type export.",
        "Qualified type names are module-surface references, not expression member paths.",
        "import or qualify an exported type alias/nominal type",
    ),
    DiagnosticExplanation {
        code: "TPZ5026",
        phase: "check",
        machine_kind: "refutable_let_pattern",
        title: "Refutable let pattern",
        summary: "A `let` pattern could fail to match at runtime.",
        why: "`let` destructuring is for irrefutable shapes; refutable cases belong in `match`, `if let`, or `while let`.",
        examples: Some(ExplainExamples {
            bad: "let Some(x) = maybe",
            good: "if let Some(x) = maybe { x } else { 0 }",
        }),
        fixits: &["use `match`/`if let`, or make the pattern irrefutable"],
    },
    explanation!(
        "TPZ5030",
        "check/package",
        "extern_declaration",
        "Extern declaration problem",
        "An extern module or function declaration is missing, malformed, duplicated, or not declared for an imported extern module.",
        "Extern FFI is declared at the package/build boundary, not inside Topaz source grammar.",
        "declare the extern module/function in topaz.toml or import an available module",
    ),
    explanation!(
        "TPZ5031",
        "check/package",
        "extern_abi_type",
        "Unsupported extern ABI type",
        "An extern function signature uses a type outside the deterministic concrete ABI allow-list.",
        "Extern calls must be replayable and content-addressed, so the ABI surface is monomorphic and deterministic.",
        "use concrete scalar, Bytes, Array, Option, Result, or unit ABI types",
    ),
    explanation!(
        "TPZ5032",
        "check/package",
        "extern_replay",
        "Invalid extern replay or sandbox binding",
        "An extern module is missing a deterministic replay fixture, has invalid replay data, or calls an entry not present in replay.",
        "v5.4 externs must execute from offline deterministic replay before any live sandbox backend is accepted.",
        "add or fix the replay fixture, and keep live extern artifacts behind the sandbox gate",
    ),
    DiagnosticExplanation {
        code: "TPZ5510",
        phase: "check",
        machine_kind: "type_argument_arity",
        title: "Wrong number of explicit type arguments",
        summary: "A call supplied a different number of type arguments than the generic declaration has parameters.",
        why: "Explicit type arguments are check-only and must align exactly with the rank-1 generic shape.",
        examples: None,
        fixits: &["remove the explicit type arguments or supply exactly one per type parameter"],
    },
    DiagnosticExplanation {
        code: "TPZ5512",
        phase: "check",
        machine_kind: "type_arguments_not_allowed",
        title: "Explicit type arguments are not allowed here",
        summary: "The callee is not a generic function/static member that accepts explicit type arguments.",
        why: "Topaz keeps call-site type arguments narrow and erased; constructors and non-generics reject them.",
        examples: None,
        fixits: &["remove the `<...>` type argument list"],
    },
    DiagnosticExplanation {
        code: "TPZ5520",
        phase: "check",
        machine_kind: "orphan_impl",
        title: "Protocol implementation violates the orphan rule",
        summary: "A protocol implementation tries to connect a protocol and type that this module does not own.",
        why: "Coherence requires one obvious implementation site for each protocol/type pair.",
        examples: None,
        fixits: &["move the impl to the protocol or type owner, or define a local wrapper type"],
    },
    DiagnosticExplanation {
        code: "TPZ5521",
        phase: "check",
        machine_kind: "duplicate_protocol_impl",
        title: "Duplicate protocol implementation",
        summary: "The same type already conforms to the same protocol.",
        why: "Protocol dispatch must be deterministic and cannot choose among overlapping implementations.",
        examples: None,
        fixits: &["remove the duplicate impl or derive clause"],
    },
    DiagnosticExplanation {
        code: "TPZ5522",
        phase: "check",
        machine_kind: "missing_protocol_conformance",
        title: "Missing protocol conformance",
        summary: "A protocol call or generic protocol bound requires a conformance the receiver type does not have.",
        why: "v5.4 protocols use static dispatch/conformance evidence, including generic bounds such as `T: Show`.",
        examples: Some(ExplainExamples {
            bad: "record User { name: string }\nfunction render<T: Show>(value: T) -> string { Show.show(value) }\nrender(User { name: \"Ada\" })",
            good: "record User derives Show { name: string }\nfunction render<T: Show>(value: T) -> string { Show.show(value) }\nrender(User { name: \"Ada\" })",
        }),
        fixits: &[
            "derive or implement the required protocol, or add a generic bound that proves it",
        ],
    },
    DiagnosticExplanation {
        code: "TPZ5523",
        phase: "check",
        machine_kind: "duplicate_protocol_bound",
        title: "Duplicate generic protocol bound",
        summary: "One type parameter repeats the same protocol in its bound conjunction.",
        why: "A bound list is a unique conjunction; silently discarding repeated syntax would hide a source mistake.",
        examples: None,
        fixits: &["remove the repeated protocol name from the bound list"],
    },
    DiagnosticExplanation {
        code: "TPZ5524",
        phase: "check/package",
        machine_kind: "non_exportable_protocol_bound",
        title: "User protocol bound cannot cross a module interface",
        summary: "An exported function exposes a bound on a module-local user protocol.",
        why: "User protocol definitions and manual witnesses are local; only Eq, Order, Show, and JSON have global interface identities.",
        examples: None,
        fixits: &["keep the function local or use one of the four predeclared protocol bounds"],
    },
    DiagnosticExplanation {
        code: "TPZ5530",
        phase: "check",
        machine_kind: "not_derivable",
        title: "Protocol cannot be derived for this type",
        summary: "A `derives` clause asks for generated protocol behavior that this type's fields or payloads cannot support.",
        why: "Derived implementations must be total and deterministic for every stored value.",
        examples: None,
        fixits: &["remove the derive or change non-derivable fields to supported types"],
    },
    DiagnosticExplanation {
        code: "TPZ5533",
        phase: "check",
        machine_kind: "not_json_encodable",
        title: "Type is not JSON encodable",
        summary: "JSON.stringify or a JSON encode path received a type outside the supported deterministic JSON shape.",
        why: "Functions, opaque foreign values, and unresolved generic shapes cannot be serialized canonically.",
        examples: None,
        fixits: &["encode a JSON-compatible record/enum/container/scalar shape"],
    },
    DiagnosticExplanation {
        code: "TPZ5534",
        phase: "check",
        machine_kind: "not_json_decodable",
        title: "Type is not JSON decodable",
        summary: "A typed JSON decode target is not a supported concrete Topaz data shape.",
        why: "Typed decode needs a fully known schema so interpreter and native decode the same bytes.",
        examples: None,
        fixits: &["decode into a concrete JSON-compatible type or JSONValue"],
    },
    DiagnosticExplanation {
        code: "TPZ5602",
        phase: "check",
        machine_kind: "duplicate_map_key",
        title: "Duplicate key in map literal",
        summary: "A map literal contains the same constant key more than once.",
        why: "Topaz map literals are deterministic and do not use last-write-wins; duplicate constant keys are rejected statically.",
        examples: Some(ExplainExamples {
            bad: "let m = map { \"a\": 1, \"a\": 2 }",
            good: "let m = map { \"a\": 2 }",
        }),
        fixits: &["remove the duplicate entry or combine the values before constructing the map"],
    },
    DiagnosticExplanation {
        code: "TPZ5610",
        phase: "check",
        machine_kind: "comprehension_body_mismatch",
        title: "Comprehension body type mismatch",
        summary: "A comprehension body does not match the element type required by its expected context.",
        why: "Expected types flow into comprehensions so empty and nested collection shapes stay precise.",
        examples: None,
        fixits: &["change the body expression or the expected collection type"],
    },
    explanation!(
        "TPZ5611",
        "check",
        "map_comprehension_body_shape",
        "Map comprehension body is not an entry",
        "A map comprehension body does not produce a key/value entry shape.",
        "Map comprehensions must produce deterministic key/value pairs for each surviving iteration.",
        "return `key: value` from the map comprehension body",
    ),
    DiagnosticExplanation {
        code: "TPZ5612",
        phase: "check",
        machine_kind: "empty_comprehension_type_needed",
        title: "Empty comprehension needs a type",
        summary: "An empty comprehension cannot infer its element, key, or value type.",
        why: "Topaz does not infer collection types from later mutations or statement order.",
        examples: Some(ExplainExamples {
            bad: "let xs = [for x in [] if false => x]",
            good: "let xs: Array<int> = [for x in [] if false => x]",
        }),
        fixits: &["add an explicit collection type annotation"],
    },
    DiagnosticExplanation {
        code: "TPZ5710",
        phase: "check",
        machine_kind: "or_pattern_binding_names",
        title: "Or-pattern alternatives bind different names",
        summary: "Every alternative of a binding or-pattern must bind the same set of names.",
        why: "The arm body must be able to use the same bindings no matter which alternative matched.",
        examples: Some(ExplainExamples {
            bad: "case Some(x) | None => x",
            good: "case Some(x) | Ok(x) => x",
        }),
        fixits: &["rename or restructure alternatives so the binding set is identical"],
    },
    DiagnosticExplanation {
        code: "TPZ5711",
        phase: "check",
        machine_kind: "or_pattern_binding_types",
        title: "Or-pattern binding types disagree",
        summary: "A name bound by multiple alternatives of an or-pattern has inconsistent types.",
        why: "The arm body sees one binding type, not a branch-dependent local type.",
        examples: None,
        fixits: &["make each alternative bind the name at the same type"],
    },
    DiagnosticExplanation {
        code: "TPZ5720",
        phase: "check",
        machine_kind: "unknown_loop_label",
        title: "Unknown loop label",
        summary: "A labeled break or continue names a loop label that is not in scope.",
        why: "Labels are lexical loop targets; they do not resolve through outer modules or values.",
        examples: None,
        fixits: &["spell the label exactly or move the break/continue inside the labeled loop"],
    },
    DiagnosticExplanation {
        code: "TPZ5721",
        phase: "check",
        machine_kind: "break_value_mismatch",
        title: "Loop break values do not agree",
        summary: "A loop expression has break values whose types cannot join to one result type.",
        why: "A value-producing loop has one expression type shared by every value break.",
        examples: None,
        fixits: &["make every value break produce the same type, or make the loop statement-only"],
    },
    DiagnosticExplanation {
        code: "TPZ5801",
        phase: "profile",
        machine_kind: "profile_disallowed_form",
        title: "Form is disallowed by the selected profile",
        summary: "The program is valid canonical Topaz, but the selected usage profile narrows this form out of its admitted surface.",
        why: "Profiles constrain authoring surfaces without creating a second language mode or changing runtime semantics.",
        examples: Some(ExplainExamples {
            bad: "let combined = parse >> validate",
            good: "let combined = (value) => validate(parse(value))",
        }),
        fixits: &[
            "follow the diagnostic rule note and rewrite using forms admitted by the selected profile",
            "use test-profile only when the source is test code that requires the canonical assert function",
        ],
    },
    DiagnosticExplanation {
        code: "TPZ6001",
        phase: "emit",
        machine_kind: "unsupported_native_construct",
        title: "Native emitter cannot lower this construct yet",
        summary: "The program is accepted by the language, but the selected native emit path lacks a proven lowering for this construct.",
        why: "Emit refuses instead of producing a binary that could diverge from interpreter semantics.",
        examples: None,
        fixits: &[
            "run with the interpreter, use the boxed backend, or implement the missing lowering",
        ],
    },
    DiagnosticExplanation {
        code: "TPZ6002",
        phase: "emit",
        machine_kind: "native_declined",
        title: "Native backend declined and fell back",
        summary: "The monomorphized native backend declined a shape that the boxed backend can still lower.",
        why: "Native specialization is an optimization layer; semantics stay on the proven boxed/runtime path when proof is incomplete.",
        examples: None,
        fixits: &["no user action is required unless this appears as a surfaced compiler bug"],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explain_code_shape_is_strict() {
        assert!(is_explain_code_shape("TPZ5602"));
        assert!(!is_explain_code_shape("TPZ56A2"));
        assert!(!is_explain_code_shape("TPZ56020"));
    }

    #[test]
    fn duplicate_map_key_has_stable_json_explanation() {
        let explanation = explain_code("TPZ5602").expect("registered");
        assert_eq!(explanation.machine_kind, "duplicate_map_key");
        assert_eq!(
            render_explain_json(explanation),
            "{\"code\":\"TPZ5602\",\"phase\":\"check\",\"machine\":{\"kind\":\"duplicate_map_key\"},\
             \"title\":\"Duplicate key in map literal\",\"summary\":\"A map literal contains the same constant key more than once.\",\
             \"why\":\"Topaz map literals are deterministic and do not use last-write-wins; duplicate constant keys are rejected statically.\",\
             \"examples\":{\"bad\":\"let m = map { \\\"a\\\": 1, \\\"a\\\": 2 }\",\"good\":\"let m = map { \\\"a\\\": 2 }\"},\
             \"fixits\":[\"remove the duplicate entry or combine the values before constructing the map\"]}"
        );
    }

    #[test]
    fn missing_protocol_bound_is_explained() {
        let explanation = explain_code("TPZ5522").expect("registered");
        assert_eq!(explanation.machine_kind, "missing_protocol_conformance");
        assert!(render_explain(explanation).contains("derive or implement"));
    }

    #[test]
    fn explanation_registry_has_unique_valid_codes() {
        let mut codes = std::collections::BTreeSet::new();
        for explanation in EXPLANATIONS {
            assert!(
                Code::has_registry_shape(explanation.code),
                "{}",
                explanation.code
            );
            assert!(
                codes.insert(explanation.code),
                "duplicate {}",
                explanation.code
            );
            assert!(!explanation.phase.is_empty(), "{} phase", explanation.code);
            assert!(
                !explanation.machine_kind.is_empty(),
                "{} machine kind",
                explanation.code
            );
        }
    }
}
