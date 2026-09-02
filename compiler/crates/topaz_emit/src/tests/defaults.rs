use super::*;

#[test]
fn a_type_alias_emits_nothing() {
    // §6 a `type` alias is a runtime no-op — it emits no statement, and the
    // program still produces its tail value.
    let src = emit_unit(&unit_of("type Foo = int\nlet x = 5\nx")).expect("emit");
    assert!(
        !src.contains("Foo") && src.contains("let _t"),
        "got:\n{src}"
    );
}

#[test]
fn emits_const_declarations() {
    // §4 a TOP-LEVEL const is HOISTED (its value is emitted before the rest),
    // so a FORWARD reference compiles: `let y = X` reads the const's `let`.
    let fwd = emit_unit(&unit_of("let y = X\nconst X = 10\ny")).expect("emit");
    let x = mangle("X");
    let y = mangle("y");
    assert!(
        fwd.find(&format!("let {x} = Value::Int(10)")).unwrap()
            < fwd.find(&format!("let {y} =")).unwrap(),
        "the hoisted const must precede the forward reference; got:\n{fwd}"
    );
    // A BLOCK-LOCAL const is an in-place immutable `let`.
    let blk = emit_unit(&unit_of("function f() { const C = 7\nC + 1 }\nf()")).expect("emit");
    assert!(
        blk.contains(&format!("let {} = Value::Int(7)", mangle("C"))),
        "got:\n{blk}"
    );
}

#[test]
fn a_shadowed_constructor_is_an_ordinary_call() {
    // `Some` bound as a local is a call OF that local (which faults
    // not-callable at runtime), NOT the constructor.
    let src = emit_unit(&unit_of("let Some = 5\nSome(1)")).expect("emit");
    assert!(
        src.contains("call_value(_t_536f6d65.clone()"),
        "got:\n{src}"
    );
    assert!(!src.contains("Value::Some(Rc::new"), "got:\n{src}");
}

#[test]
fn emits_a_record_update() {
    // §8 `base { field: value }` → the two shared leaves, base checked
    // before the fields evaluate.
    let src = emit_unit(&unit_of("let r = { x: 1, y: 2 }\nr { x: 9 }")).expect("emit");
    assert!(
        src.contains("record_update_base(_t_72.clone(),"),
        "got:\n{src}"
    );
    assert!(
        src.contains("record_update_merge(__base, vec![(\"x\".to_string(), Value::Int(9))],"),
        "got:\n{src}"
    );
}

#[test]
fn imported_nominal_record_literal_default_uses_defining_source() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User as Person }
let u = Person { name: "Ada" }
u.age
"#,
            ),
            (
                "model.tpz",
                r#"
export record User { name: string, age: int = 36 }
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(src.contains("Value::Int(36)"), "got:\n{src}");
}

#[test]
fn imported_nominal_record_reference_default_uses_defining_exported_const() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User as Person }
let DEFAULT_AGE = 999
let u = Person { name: "Ada" }
u.age
"#,
            ),
            (
                "model.tpz",
                r#"
export const DEFAULT_AGE = 36
export record User { name: string, age: int = DEFAULT_AGE }
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(src.contains("Value::Int(36)"), "got:\n{src}");
}

#[test]
fn imported_nominal_record_reference_default_uses_defining_private_const() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { Widget }
let scale = 999
let w = Widget { label: "bolt" }
w.size
"#,
            ),
            (
                "model.tpz",
                r#"
const scale = 12
export record Widget { label: string, size: int = scale * 3 }
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(src.contains("Value::Int(36)"), "got:\n{src}");
    assert!(src.contains("Value::Int(999)"), "got:\n{src}");
}

#[test]
fn imported_nominal_record_reference_default_uses_selected_imported_const() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User as Person }
let importedBase = 999
let u = Person { name: "Ada" }
u.age
"#,
            ),
            (
                "model.tpz",
                r#"
import config { base as importedBase }
export record User { name: string, age: int = importedBase * 2 }
"#,
            ),
            (
                "config.tpz",
                r#"
export const base = 18
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(src.contains("Value::Int(36)"), "got:\n{src}");
}

#[test]
fn imported_nominal_record_reference_default_uses_namespace_imported_const() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User as Person }
let config = { base: 999 }
let u = Person { name: "Ada" }
u.age
"#,
            ),
            (
                "model.tpz",
                r#"
import config
export record User { name: string, age: int = config.base * 2 }
"#,
            ),
            (
                "config.tpz",
                r#"
export const base = 18
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(src.contains("Value::Int(36)"), "got:\n{src}");
}

#[test]
fn imported_nominal_record_reference_default_uses_selected_imported_runtime_value() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User as Person }
let base = 999
let u = Person { name: "Ada" }
u.age
"#,
            ),
            (
                "model.tpz",
                r#"
import config { base }
export record User { name: string, age: int = base }
"#,
            ),
            (
                "config.tpz",
                r#"
export let base = 36
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(
        src.contains("member_value_required(&__mod__t_636f6e666967, \"base\""),
        "got:\n{src}"
    );
    assert!(src.contains("Value::Int(999)"), "got:\n{src}");
}

#[test]
fn imported_nominal_record_reference_default_uses_namespace_imported_runtime_value() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User as Person }
let config = { base: 999 }
let u = Person { name: "Ada" }
u.age
"#,
            ),
            (
                "model.tpz",
                r#"
import config
export record User { name: string, age: int = config.base }
"#,
            ),
            (
                "config.tpz",
                r#"
export let base = 36
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(
        src.contains("member_value_required(&__mod__t_636f6e666967, \"base\""),
        "got:\n{src}"
    );
    assert!(src.contains("Value::Int(999)"), "got:\n{src}");
}

#[test]
fn imported_nominal_record_reference_default_uses_namespace_runtime_record_field() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User as Person }
let config = { inner: { base: 999 } }
let u = Person { name: "Ada" }
u.age
"#,
            ),
            (
                "model.tpz",
                r#"
import config
export record User { name: string, age: int = config.inner.base }
"#,
            ),
            (
                "config.tpz",
                r#"
export let inner = { base: 36 }
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(
        src.contains(
            "member_value_required(&(member_value_required(&__mod__t_636f6e666967, \"inner\""
        ),
        "got:\n{src}"
    );
    assert!(src.contains("), \"base\", Span::new"), "got:\n{src}");
    assert!(src.contains("Value::Int(999)"), "got:\n{src}");
}

#[test]
fn imported_nominal_record_reference_default_uses_namespace_private_runtime_value() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User as Person }
let config = { base: 999 }
let u = Person { name: "Ada" }
u.age + u.localAge + config.base
"#,
            ),
            (
                "model.tpz",
                r#"
import config as cfg
let base: int = 7
export record User { name: string, age: int = cfg.base, localAge: int = base }
"#,
            ),
            (
                "config.tpz",
                r#"
let base: int = 36
export const marker = 0
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(
        src.contains("(\"\\0topaz.self-default.config.base\".to_string(), _t_62617365.clone())"),
        "got:\n{src}"
    );
    assert!(
        src.contains("(\"\\0topaz.self-default.model.base\".to_string(), _t_62617365.clone())"),
        "got:\n{src}"
    );
    assert!(
        src.contains(
            "member_value_required(&__mod__t_636f6e666967, \"\\0topaz.self-default.config.base\""
        ),
        "got:\n{src}"
    );
    assert!(src.contains("Value::Int(999)"), "got:\n{src}");
}

#[test]
fn imported_nominal_record_reference_default_uses_namespace_private_runtime_record_field() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User as Person }
let cfg = { inner: { base: 999 } }
let u = Person { name: "Ada" }
u.age + cfg.inner.base
"#,
            ),
            (
                "model.tpz",
                r#"
import config as cfg
export record User { name: string, age: int = cfg.inner.base }
"#,
            ),
            (
                "config.tpz",
                r#"
let inner = { base: 36 }
export const marker = 0
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(
        src.contains("(\"\\0topaz.self-default.config.inner\".to_string(), _t_696e6e6572.clone())"),
        "got:\n{src}"
    );
    assert!(
        src.contains(
            "member_value_required(&(member_value_required(&__mod__t_636f6e666967, \"\\0topaz.self-default.config.inner\""
        ),
        "got:\n{src}"
    );
    assert!(src.contains("), \"base\", Span::new"), "got:\n{src}");
    assert!(src.contains("Value::Int(999)"), "got:\n{src}");
}

#[test]
fn imported_nominal_record_reference_default_uses_deep_namespace_private_runtime_record_field() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User as Person }
let cfg = { inner: { deep: { base: 999 } } }
let u = Person { name: "Ada" }
u.age + cfg.inner.deep.base
"#,
            ),
            (
                "model.tpz",
                r#"
import config as cfg
export record User { name: string, age: int = cfg.inner.deep.base }
"#,
            ),
            (
                "config.tpz",
                r#"
let inner = { deep: { base: 36 } }
export const marker = 0
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(
        src.contains("(\"\\0topaz.self-default.config.inner\".to_string(), _t_696e6e6572.clone())"),
        "got:\n{src}"
    );
    assert!(
        src.contains(
            "member_value_required(&(member_value_required(&(member_value_required(&__mod__t_636f6e666967, \"\\0topaz.self-default.config.inner\""
        ),
        "got:\n{src}"
    );
    assert!(src.contains("), \"deep\", Span::new"), "got:\n{src}");
    assert!(src.contains("), \"base\", Span::new"), "got:\n{src}");
    assert!(src.contains("Value::Int(999)"), "got:\n{src}");
}

#[test]
fn imported_nominal_record_reference_default_uses_own_exported_runtime_value() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User as Person }
let base = 999
let u = Person { name: "Ada" }
u.age + base
"#,
            ),
            (
                "model.tpz",
                r#"
export let base = 36
export record User { name: string, age: int = base }
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(
        src.contains("member_value_required(&__mod__t_6d6f64656c, \"base\""),
        "got:\n{src}"
    );
    assert!(src.contains("Value::Int(999)"), "got:\n{src}");
}

#[test]
fn imported_nominal_record_reference_default_uses_own_private_runtime_value() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User as Person }
let base = 999
record Local { name: string, age: int = base }
function main() -> string {
let u = Person { name: "Ada" }
let l = Local { name: "Entry" }
"{u.age}:{l.age}:{base}"
}
main()
"#,
            ),
            (
                "model.tpz",
                r#"
let base = 36
export record User { name: string, age: int = base }
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(
        src.contains("(\"\\0topaz.self-default.model.base\".to_string(), _t_62617365.clone())"),
        "got:\n{src}"
    );
    assert!(
        src.contains(
            "member_value_required(&__mod__t_6d6f64656c, \"\\0topaz.self-default.model.base\""
        ),
        "got:\n{src}"
    );
    assert!(
        src.contains("__topaz_self_default__t_6d61696e__t_62617365"),
        "entry self default cell must be module-qualified:\n{src}"
    );
    assert!(
        src.contains("__topaz_self_default__t_6d6f64656c__t_62617365"),
        "model self default cell must be module-qualified:\n{src}"
    );
}

#[test]
fn nominal_record_reference_default_uses_root_self_runtime_value() {
    let unit = unit_of(
        r#"
let base = 36
record User { name: string, age: int = base }
function main() -> string {
let base = 999
let u = User { name: "Ada" }
"{u.age}:{base}"
}
main()
"#,
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(
        src.contains("let __topaz_self_default__t_6d61696e__t_62617365 = top_cell();"),
        "got:\n{src}"
    );
    assert!(
        src.contains(
            "top_cell_set(&__topaz_self_default__t_6d61696e__t_62617365, _t_62617365.clone());"
        ),
        "got:\n{src}"
    );
    assert!(
        src.contains("top_cell_get(&__topaz_self_default__t_6d61696e__t_62617365, \"base\""),
        "got:\n{src}"
    );
    assert!(src.contains("Value::Int(999)"), "got:\n{src}");
}

#[test]
fn nominal_record_reference_default_seed_is_visible_to_earlier_functions() {
    let unit = unit_of(
        r#"
function make() -> int {
let u = User { name: "Ada" }
u.age
}
let base = 36
record User { name: string, age: int = base }
make()
"#,
    );
    let src = emit_unit(&unit).expect("emit");
    assert!(
        src.find("let __topaz_self_default__t_6d61696e__t_62617365 = top_cell();")
            .unwrap()
            < src.find("top_cell_set(&_t_6d616b65").unwrap(),
        "self default seed must be in scope before earlier function bodies lower:\n{src}"
    );
    assert!(
        src.contains("top_cell_get(&__topaz_self_default__t_6d61696e__t_62617365, \"base\""),
        "got:\n{src}"
    );
}

#[test]
fn nominal_record_forward_runtime_default_declines_loudly() {
    let unit = unit_of(
        r#"
record User { name: string, age: int = base }
let base = 36
let u = User { name: "Ada" }
u.age
"#,
    );
    let error = emit_unit(&unit).expect_err("forward runtime default should decline");
    match error.kind {
        EmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "imported nominal record reference default")
        }
        other => panic!("expected unsupported forward runtime default, got {other:?}"),
    }
    let span = error.span.expect("decline should carry a span");
    assert_eq!(text(&unit.modules[0].text, span).trim(), "base");
}

#[test]
fn nominal_record_mut_runtime_default_emits_defining_scope_thunk() {
    let unit = unit_of(
        r#"
let mut base = 36
record User { name: string, age: int = base }
base = 41
let u = User { name: "Ada" }
u.age
"#,
    );
    let src = emit_unit(&unit).expect("mutable runtime default should emit");
    assert!(
        src.contains("__topaz_record_default__t_6d61696e__t_55736572__t_616765"),
        "got:\n{src}"
    );
    assert!(src.contains("call_value(top_cell_get("), "got:\n{src}");
}

#[test]
fn nominal_record_effectful_default_emits_call_time_thunk() {
    let unit = unit_of(
        r#"
function mark(value: int) -> int {
print("{value}")
value
}
record User { age: int = mark(36) }
let explicit = User { age: 9 }
let omitted = User {}
explicit.age + omitted.age
"#,
    );
    let src = emit_unit(&unit).expect("effectful record default should emit");
    assert!(
        src.contains("User.age default") && src.contains("call_value(top_cell_get("),
        "got:\n{src}"
    );
    assert_eq!(
        src.matches("let __d0: Value = call_value(top_cell_get(")
            .count(),
        1,
        "shared initialization should contain the omitted construction once:\n{src}"
    );
}

#[test]
fn imported_nominal_record_mut_runtime_default_emits_hidden_thunk() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User, setBase }
setBase(41)
let u = User { name: "Ada" }
u.age
"#,
            ),
            (
                "model.tpz",
                r#"
let mut base = 36
export function setBase(value: int) -> unit {
base = value
}
export record User { name: string, age: int = base }
"#,
            ),
        ],
    );
    let src = emit_unit(&unit).expect("imported mutable runtime default should emit");
    assert!(
        src.contains("__topaz_record_default::model::User::age"),
        "got:\n{src}"
    );
    assert!(
        src.contains("member_value_required(&__mod__t_6d6f64656c, \"__topaz_record_default::model::User::age\""),
        "got:\n{src}"
    );
}

#[test]
fn imported_nominal_record_namespace_mut_runtime_record_field_default_declines_loudly() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User as Person }
let u = Person { name: "Ada" }
u.age
"#,
            ),
            (
                "model.tpz",
                r#"
import config as cfg
export record User { name: string, age: int = cfg.inner.base }
"#,
            ),
            (
                "config.tpz",
                r#"
let mut inner = { base: 36 }
export const marker = 0
"#,
            ),
        ],
    );
    let error = emit_unit(&unit).expect_err("mutable namespace runtime default should decline");
    match error.kind {
        EmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "imported nominal record reference default")
        }
        other => panic!("expected unsupported runtime record-field default, got {other:?}"),
    }
    let span = error.span.expect("decline should carry a span");
    let model_src = unit
        .modules
        .iter()
        .find(|module| module.identity == "model")
        .map(|module| &module.text)
        .expect("model source");
    assert_eq!(text(model_src, span).trim(), "cfg.inner.base");
}

#[test]
fn imported_nominal_record_namespace_optional_default_declines_loudly() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { User }
let u = User { name: "Ada" }
u.age
"#,
            ),
            (
                "model.tpz",
                r#"
import config
export record User { name: string, age: int = config?.base }
"#,
            ),
            (
                "config.tpz",
                r#"
export const base = 36
"#,
            ),
        ],
    );
    let error = emit_unit(&unit).expect_err("optional namespace default should decline");
    match error.kind {
        EmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "imported nominal record reference default")
        }
        other => panic!("expected optional namespace record default decline, got {other:?}"),
    }
    let span = error.span.expect("decline should carry a span");
    let model_src = unit
        .modules
        .iter()
        .find(|module| module.identity == "model")
        .map(|module| &module.text)
        .expect("model source");
    assert_eq!(text(model_src, span).trim(), "config?.base");
}

#[test]
fn top_level_const_collection_does_not_fold_namespace_members() {
    let unit = unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import config
const value = config.base
0
"#,
            ),
            (
                "config.tpz",
                r#"
export const base = 36
"#,
            ),
        ],
    );
    let entry = unit
        .modules
        .iter()
        .find(|module| module.is_entry)
        .expect("entry module");
    let src = &entry.text;
    let declarations = collect_local_declaration_inventory(
        &entry.program.items,
        src,
        &entry.identity,
        "",
        unit.language_version >= topaz_syntax::LangVersion::V5_20,
    );
    let modules = std::collections::BTreeMap::new();
    let namespaces = std::collections::BTreeMap::new();
    let ModuleDefaultInputFacts {
        const_values: consts,
        exported_const_values: exported_consts,
        ..
    } = collect_module_default_input_facts(
        &entry.program.items,
        src,
        &entry.identity,
        &modules,
        &namespaces,
        &declarations.top_binding_cardinality,
    );
    assert!(
        !consts.iter().any(|(name, _)| name == "value") && exported_consts.is_empty(),
        "top-level namespace members must not enter ordinary const folding: {consts:?}"
    );
}

#[test]
fn a_record_update_capturing_enclosing_bindings_emits() {
    // The capture walkers descend into the base and field values of a
    // record update (the checklist for a new emittable construct).
    emit_unit(&unit_of(
        "let r = { n: 1 }\nlet v = 5\nlet f = () => r { n: v }\nf()",
    ))
    .expect("emit");
}
