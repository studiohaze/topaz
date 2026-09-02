use super::*;

#[test]
fn emits_same_module_nominal_record_construction_through_checked_helper() {
    let generated = emit_source(
        r#"
record User { name: string, age: int = 36 }
function main() -> int {
    let u = User { name: "Ada" }
    let v = User { ...u, age: 37 }
    v.age
}
main()
"#,
    );
    assert!(generated.contains("class _tnr_t_55736572:"), "{generated}");
    assert!(
        generated.contains("__topaz_record_id__ = \"User\""),
        "{generated}"
    );
    assert!(
        generated.contains(
            "__topaz_record_fields__ = ((\"_t_6e616d65\", \"name\"), (\"_t_616765\", \"age\"))"
        ),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_nominal_record(_tnr_t_55736572, \"User\", "),
        "{generated}"
    );
    assert!(generated.contains("lambda: 36"), "{generated}");
    assert!(generated.contains("lambda: _t_75"), "{generated}");
    assert!(
        generated.contains("(\"_t_616765\", \"age\", lambda: 37)"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nominal record Python gate failed: {e}"));
}

#[test]
fn emits_nominal_record_match_patterns() {
    let generated = emit_source(
        r#"
record User { name: string, age: int }
function main() -> string {
    let u = User { name: "Ada", age: 36 }
    match u {
        case User { name, age: years } => print("{name}:{years}")
    }
    match u {
        case User { name: n, age } => "{n}:{age}"
    }
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_is_nominal_record(__tpz_match"),
        "{generated}"
    );
    assert!(generated.contains("_tnr_t_55736572"), "{generated}");
    assert!(generated.contains("._t_6e616d65"), "{generated}");
    assert!(generated.contains("._t_616765"), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nominal record pattern Python gate failed: {e}"));
}

#[test]
fn exported_top_level_values_and_functions_emit_like_plain_items() {
    let generated = emit_source(
        r#"
export const base = 40
export let extra = 2
export function answer() -> int {
    base + extra
}
answer()
"#,
    );
    assert!(
        generated.contains("def _t_616e73776572(host):  # answer"),
        "{generated}"
    );
    assert!(
        generated.contains("globals()[\"_t_62617365\"] = 40  # base"),
        "{generated}"
    );
    assert!(
        generated.contains("globals()[\"_t_6578747261\"] = 2  # extra"),
        "{generated}"
    );
    assert!(
        generated.contains("__tpz_value = _t_616e73776572(host)"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("generated Python gate failed: {e}"));
}

#[test]
fn record_shape_collection_reads_non_entry_file_spans() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "main.tpz",
        r#"
import util
0
"#,
    );
    provider.add_file(
        "util.tpz",
        r#"
export function boxed() -> int {
    let row = { payload: 1 }
    row.payload
}
"#,
    );
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    assert!(
        unit.diagnostics.is_empty(),
        "fixture must resolve cleanly: {:?}",
        unit.diagnostics
    );
    let shapes = super::collect_record_shapes(&unit);
    assert!(
        shapes
            .iter()
            .any(|shape| shape.fields == vec!["payload".to_string()]),
        "non-entry module record shape was not collected: {shapes:?}"
    );
}

#[test]
fn record_shape_collection_traverses_comprehension_bodies() {
    let generated = emit_source(
        r#"
function main() -> string {
    let rows = [ for x in [1, 2] => { payload: x } ]
    "{rows[0].payload}:{rows[1].payload}"
}
main()
"#,
    );
    assert!(
        generated.contains("class _tr_7061796c6f6164:"),
        "comprehension body record literal must predeclare its shape: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("comprehension record shape gate failed: {e}"));
}

#[test]
fn record_shape_collection_traverses_pipe_operands() {
    let generated = emit_source(
        r#"
function main() -> int {
    let value = ({ payload: 7 }) |> ((row) => row.payload)
    value
}
main()
"#,
    );
    assert!(
        generated.contains("class _tr_7061796c6f6164:"),
        "pipe operand record literal must predeclare its shape: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("pipe record shape gate failed: {e}"));
}

#[test]
fn record_shape_collection_traverses_string_interpolations() {
    let generated = emit_source(
        r#"
function main() -> string {
    "{({ payload: 3 }).payload}"
}
main()
"#,
    );
    assert!(
        generated.contains("class _tr_7061796c6f6164:"),
        "string interpolation record literal must predeclare its shape: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("string interpolation record shape gate failed: {e}"));
}

#[test]
fn record_shape_collection_traverses_range_operands() {
    let generated = emit_source(
        r#"
function main() -> string {
    let values = ({ lo: 1 }).lo..3 by ({ step: 1 }).step
    "{values}"
}
main()
"#,
    );
    assert!(
        generated.contains("class _tr_6c6f:"),
        "range low endpoint record literal must predeclare its shape: {generated}"
    );
    assert!(
        generated.contains("class _tr_73746570:"),
        "range step record literal must predeclare its shape: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("range record shape gate failed: {e}"));
}

#[test]
fn record_shape_collection_traverses_loop_break_values() {
    let generated = emit_source(
        r#"
function main() -> int {
    let row = loop {
        break { breakPayload: 40, breakBonus: 2 }
    }
    row.breakPayload + row.breakBonus
}
main()
"#,
    );
    assert!(
        generated.contains("class _tr_627265616b5061796c6f6164_627265616b426f6e7573:"),
        "loop break record literal must predeclare its shape: {generated}"
    );
    assert_generated_python_ok_int(&generated, 42, "loop break record shape");
}

#[test]
fn map_entries_predeclares_entry_record_and_map_of_entries_lowers() {
    let generated = emit_source(
        r#"
function main() -> int {
    let rows = map { "a": 1 }.entries
    let m: Map<string, int> = rows |> Map.ofEntries()
    m.getOr("a", 0)
}
main()
"#,
    );
    assert!(
        generated.contains("class _tr_6b6579_76616c7565:"),
        "Map.entries must predeclare the {{key,value}} record shape: {generated}"
    );
    assert!(
        generated.contains("tpz_member(") && generated.contains("\"entries\""),
        "Map.entries must lower through tpz_member: {generated}"
    );
    assert!(
        generated.contains("tpz_map_of_entries("),
        "Map.ofEntries must lower to the Python runtime helper: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("Map.entries/ofEntries Python gate failed: {e}"));
}

#[test]
fn imported_module_values_emit_namespace_and_selected_python_surfaces() {
    let namespace = emit_source_with_files(
        r#"
import util
print(util.answer)
"#,
        &[("util.tpz", "export const answer = 42\n")],
    );
    assert!(
        namespace.contains("class _tpz_ns__t_7574696c:"),
        "{namespace}"
    );
    assert!(
        namespace.contains("globals()[\"_tpz_mod__t_7574696c___t_616e73776572\"] = 42  # answer"),
        "{namespace}"
    );
    assert!(
        namespace.contains("globals()[\"_t_7574696c\"] = _tpz_mod__t_7574696c  # util"),
        "{namespace}"
    );
    assert!(
        namespace.contains("tpz_member(_t_7574696c, \"_t_616e73776572\", \"answer\""),
        "{namespace}"
    );
    assert_generated_python_gates(&namespace)
        .unwrap_or_else(|e| panic!("namespace import Python gate failed: {e}"));

    let selected = emit_source_with_files(
        r#"
import util { answer as final }
print(final)
"#,
        &[("util.tpz", "export let answer = 42\n")],
    );
    assert!(
        selected.contains(
            "globals()[\"_t_66696e616c\"] = _tpz_mod__t_7574696c___t_616e73776572  # final"
        ),
        "{selected}"
    );
    assert!(selected.contains("host.print(_t_66696e616c"), "{selected}");
    assert_generated_python_gates(&selected)
        .unwrap_or_else(|e| panic!("selected import Python gate failed: {e}"));
}

#[test]
fn top_level_const_annotations_drive_entry_and_imported_value_metadata() {
    let generated = emit_source_with_files(
        r#"
import util
const entryText: string = "한"
function main() -> int {
    entryText.byteLength() + util.value.byteLength()
}
main()
"#,
        &[("util.tpz", "export const value: string = \"AZ\"\n")],
    );
    assert_generated_python_ok_int(
        &generated,
        5,
        "entry and imported top-level const declared-type metadata parity",
    );
}

#[test]
fn imported_nominal_records_emit_selected_constructors_and_patterns() {
    let selected = emit_source_with_files(
        r#"
import model { User as Person }
function main() -> string {
    let u = Person { name: "Ada" }
    match u {
        case Person { name, age } => "{name}:{age}"
    }
}
main()
"#,
        &[(
            "model.tpz",
            r#"
export record User { name: string, age: int = 36 }
"#,
        )],
    );
    assert!(
        selected.contains("class _tnr_t_6d6f64656c___t_55736572:"),
        "{selected}"
    );
    assert!(
        selected.contains("tpz_nominal_record(_tnr_t_6d6f64656c___t_55736572, \"User\", "),
        "{selected}"
    );
    assert!(selected.contains("lambda: 36"), "{selected}");
    assert!(
        selected.contains("tpz_is_nominal_record(__tpz_match, \"User\")"),
        "{selected}"
    );
    assert_generated_python_gates(&selected)
        .unwrap_or_else(|e| panic!("selected imported record Python gate failed: {e}"));
}

#[test]
fn imported_nominal_record_defaults_use_selected_imported_consts() {
    let generated = emit_source_with_files(
        r#"
import model { User as Person }
function main() -> string {
    let importedBase = 999
    let u = Person { name: "Ada" }
    "{u.age}:{importedBase}"
}
main()
"#,
        &[
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
    assert!(generated.contains("lambda: 36"), "{generated}");
    assert!(
        generated.contains("_t_696d706f7274656442617365 = 999"),
        "{generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("selected imported const record default Python gate failed: {e}")
    });
}

#[test]
fn imported_nominal_record_defaults_use_private_consts() {
    let generated = emit_source_with_files(
        r#"
import model { Widget }
function main() -> string {
    let scale = 999
    let w = Widget { label: "bolt" }
    "{w.size}:{scale}"
}
main()
"#,
        &[(
            "model.tpz",
            r#"
const scale = 12
export record Widget { label: string, size: int = scale * 3 }
"#,
        )],
    );
    assert!(generated.contains("lambda: 36"), "{generated}");
    assert!(generated.contains("_t_7363616c65 = 999"), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("private const record default Python gate failed: {e}"));
}

#[test]
fn imported_nominal_record_defaults_use_selected_imported_runtime_values() {
    let generated = emit_source_with_files(
        r#"
import model { User }
function main() -> int {
    let base = 999
    let u = User { name: "Ada" }
    u.age + base
}
main()
"#,
        &[
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
    assert!(
        generated.contains("lambda: _tpz_mod__t_636f6e666967___t_62617365"),
        "{generated}"
    );
    assert_generated_source_assignment(&generated, "base", "999");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("selected imported runtime default Python gate failed: {e}"));
}

#[test]
fn imported_nominal_record_defaults_use_own_exported_runtime_values() {
    let generated = emit_source_with_files(
        r#"
import model { User as Person }
function main() -> int {
    let base = 999
    let u = Person { name: "Ada" }
    u.age + base
}
main()
"#,
        &[(
            "model.tpz",
            r#"
export let base = 36
export record User { name: string, age: int = base }
"#,
        )],
    );
    assert!(
        generated.contains("lambda: _tpz_mod__t_6d6f64656c___t_62617365"),
        "{generated}"
    );
    assert_generated_source_assignment(&generated, "base", "999");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("own exported runtime default Python gate failed: {e}"));
}

#[test]
fn imported_nominal_record_defaults_use_namespace_imported_consts() {
    let generated = emit_source_with_files(
        r#"
import model { User }
function main() -> int {
    let u = User { name: "Ada" }
    u.age
}
main()
"#,
        &[
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
export const base = 36
"#,
            ),
        ],
    );
    assert!(generated.contains("lambda: 36"), "{generated}");
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("namespace imported const record default Python gate failed: {e}")
    });
}

#[test]
fn imported_nominal_record_defaults_use_namespace_imported_runtime_values() {
    let generated = emit_source_with_files(
        r#"
import model { User as Person }
function main() -> int {
    let config = { base: 999 }
    let u = Person { name: "Ada" }
    u.age
}
main()
"#,
        &[
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
    assert!(
        generated.contains("lambda: _tpz_mod__t_636f6e666967___t_62617365"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("namespace imported runtime default Python gate failed: {e}"));
}

#[test]
fn imported_nominal_record_defaults_use_namespace_runtime_record_fields() {
    let generated = emit_source_with_files(
        r#"
import model { User as Person }
function main() -> string {
    let config = { inner: { base: 999 } }
    let u = Person { name: "Ada" }
    "{u.age}:{config.inner.base}"
}
main()
"#,
        &[
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
    assert!(
        generated.contains(
            "tpz_member(_tpz_mod__t_636f6e666967___t_696e6e6572, \"_t_62617365\", \"base\""
        ),
        "{generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("namespace runtime record-field default Python gate failed: {e}")
    });
}

#[test]
fn nominal_record_defaults_use_root_self_runtime_values() {
    let generated = emit_source(
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
    assert!(
        generated.contains(
            "globals()[\"__topaz_self_default__t_6d61696e__t_62617365\"] = __tpz_missing"
        ),
        "{generated}"
    );
    assert!(
        generated.contains(
            "globals()[\"__topaz_self_default__t_6d61696e__t_62617365\"] = _t_62617365  # base"
        ),
        "{generated}"
    );
    assert!(
        generated.contains(
            "lambda: __tpz_self_default(__topaz_self_default__t_6d61696e__t_62617365, \"base\","
        ),
        "{generated}"
    );
    assert_generated_source_assignment(&generated, "base", "999");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("root self runtime default Python gate failed: {e}"));
}

#[test]
fn imported_nominal_record_defaults_use_own_private_runtime_values() {
    let generated = emit_source_with_files(
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
        &[(
            "model.tpz",
            r#"
let base = 36
export record User { name: string, age: int = base }
"#,
        )],
    );
    assert!(
        generated.contains(
            "globals()[\"__topaz_self_default__t_6d6f64656c__t_62617365\"] = __tpz_missing"
        ),
        "{generated}"
    );
    assert!(
        generated.contains(
            "globals()[\"__topaz_self_default__t_6d61696e__t_62617365\"] = __tpz_missing"
        ),
        "{generated}"
    );
    assert!(
        generated.contains(
            "lambda: __tpz_self_default(__topaz_self_default__t_6d6f64656c__t_62617365, \"base\","
        ),
        "{generated}"
    );
    assert!(
        generated.contains(
            "lambda: __tpz_self_default(__topaz_self_default__t_6d61696e__t_62617365, \"base\","
        ),
        "{generated}"
    );
    assert!(
        generated.contains("globals()[\"_t_62617365\"] = 999  # base"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("own private runtime default Python gate failed: {e}"));
}

#[test]
fn nominal_record_runtime_default_seed_is_visible_to_earlier_functions() {
    let generated = emit_source(
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
    assert!(
        generated.contains(
            "lambda: __tpz_self_default(__topaz_self_default__t_6d61696e__t_62617365, \"base\","
        ),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("earlier function self runtime default gate failed: {e}"));
}

#[test]
fn nominal_record_forward_runtime_default_declines_loudly() {
    let error = emit_error_for_source(
        r#"
record User { name: string, age: int = base }
let base = 36
function main() -> int {
    let u = User { name: "Ada" }
    u.age
}
main()
"#,
    );
    assert_eq!(error.code(), "TPZ6PY0001");
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "imported nominal record reference default")
        }
        other => panic!("expected forward runtime record default decline, got {other:?}"),
    }
    assert!(
        error.span.is_some(),
        "forward runtime record default decline should carry a span"
    );
}

#[test]
fn nominal_record_mut_runtime_default_reads_current_binding() {
    let generated = emit_source(
        r#"
let mut base = 36
record User { name: string, age: int = base }
base = 41
function main() -> int {
    let u = User { name: "Ada" }
    u.age
}
main()
"#,
    );
    assert_generated_python_ok_int(&generated, 41, "mutable nominal record default");
}

#[test]
fn nominal_record_effectful_default_runs_once_only_when_omitted() {
    let generated = emit_source(
        r#"
function mark(value: int) -> int {
    print("{value}")
    value
}
record User { age: int = mark(36) }
function main() -> int {
    let explicit = User { age: 9 }
    let omitted = User {}
    explicit.age + omitted.age
}
main()
"#,
    );
    assert_generated_python_ok_int_with_files_and_stdout(
        &generated,
        45,
        &[],
        &["36"],
        "effectful nominal record default",
    );
}

#[test]
fn nominal_record_effectful_default_uses_cooperative_helper() {
    let generated = emit_source(
        r#"
function mark(value: int) -> int {
    print("{value}")
    value
}
record User { age: int = mark(36) }
function make() -> int {
    let user = User {}
    user.age
}
function main() -> int {
    let result = concurrent {
        value: make()
        other: 0
    }
    result.value + result.other
}
main()
"#,
    );
    assert!(
        generated.contains("yield from tpz_nominal_record__co("),
        "{generated}"
    );
    assert_generated_python_ok_int_with_files_and_stdout(
        &generated,
        36,
        &[],
        &["36"],
        "cooperative effectful nominal record default",
    );
}

#[test]
fn imported_nominal_record_mut_runtime_default_reads_defining_module_binding() {
    let generated = emit_source_with_files(
        r#"
import model { User, setBase }
function main() -> int {
    setBase(41)
    let user = User {}
    user.age
}
main()
"#,
        &[(
            "model.tpz",
            r#"
let mut base = 36
export function setBase(value: int) -> unit {
    base = value
}
export record User { age: int = base }
"#,
        )],
    );
    assert_generated_python_ok_int(&generated, 41, "imported mutable nominal record default");
}

#[test]
fn imported_nominal_record_namespace_private_runtime_default_emits() {
    let module = emit_source_with_files(
        r#"
import model { User as Person }
let config = { base: 999 }
function main() -> int {
    let u = Person { name: "Ada" }
    u.age + u.localAge + config.base
}
main()
"#,
        &[
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
    assert!(
        module.contains(
            "globals()[\"__topaz_self_default__t_636f6e666967__t_62617365\"] = __tpz_missing"
        ),
        "got:\n{module}"
    );
    assert!(
        module.contains(
            "globals()[\"__topaz_self_default__t_636f6e666967__t_62617365\"] = _tpz_mod__t_636f6e666967___t_62617365  # base"
        ),
        "got:\n{module}"
    );
    assert!(
        module.contains(
            "lambda: __tpz_self_default(__topaz_self_default__t_636f6e666967__t_62617365, \"cfg.base\","
        ),
        "got:\n{module}"
    );
    assert!(
        module.contains(
            "lambda: __tpz_self_default(__topaz_self_default__t_6d6f64656c__t_62617365, \"base\","
        ),
        "got:\n{module}"
    );
}

#[test]
fn imported_nominal_record_namespace_private_runtime_record_field_default_emits() {
    let module = emit_source_with_files(
        r#"
import model { User as Person }
let cfg = { inner: { base: 999 } }
function main() -> int {
    let u = Person { name: "Ada" }
    u.age + cfg.inner.base
}
main()
"#,
        &[
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
    assert!(
        module.contains(
            "globals()[\"__topaz_self_default__t_636f6e666967__t_696e6e6572\"] = __tpz_missing"
        ),
        "got:\n{module}"
    );
    assert!(
        module.contains(
            "globals()[\"__topaz_self_default__t_636f6e666967__t_696e6e6572\"] = _tpz_mod__t_636f6e666967___t_696e6e6572  # inner"
        ),
        "got:\n{module}"
    );
    assert!(
        module.contains(
            "lambda: tpz_member(__tpz_self_default(__topaz_self_default__t_636f6e666967__t_696e6e6572, \"cfg.inner\","
        ),
        "got:\n{module}"
    );
    assert!(
        module.contains("\"_t_62617365\", \"base\""),
        "got:\n{module}"
    );
    assert!(module.contains("999"), "got:\n{module}");
}

#[test]
fn imported_nominal_record_namespace_private_deep_runtime_record_field_default_emits() {
    let module = emit_source_with_files(
        r#"
import model { User as Person }
let cfg = { inner: { deep: { base: 999 } } }
function main() -> int {
    let u = Person { name: "Ada" }
    u.age + cfg.inner.deep.base
}
main()
"#,
        &[
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
    assert!(
        module.contains(
            "globals()[\"__topaz_self_default__t_636f6e666967__t_696e6e6572\"] = _tpz_mod__t_636f6e666967___t_696e6e6572  # inner"
        ),
        "got:\n{module}"
    );
    assert!(
        module.contains(
            "lambda: tpz_member(tpz_member(__tpz_self_default(__topaz_self_default__t_636f6e666967__t_696e6e6572, \"cfg.inner\","
        ),
        "got:\n{module}"
    );
    assert!(
        module.contains("\"_t_64656570\", \"deep\""),
        "got:\n{module}"
    );
    assert!(
        module.contains("\"_t_62617365\", \"base\""),
        "got:\n{module}"
    );
}

#[test]
fn imported_nominal_record_namespace_mut_runtime_record_field_default_declines_loudly() {
    let error = emit_unchecked_error_for_source_with_files(
        r#"
import model { User }
function main() -> int {
    let u = User { name: "Ada" }
    u.age
}
main()
"#,
        &[
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
    assert_eq!(error.code(), "TPZ6PY0001");
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "imported nominal record reference default")
        }
        other => {
            panic!("expected namespace mutable record-field default decline, got {other:?}")
        }
    }
    assert!(
        error.span.is_some(),
        "namespace mutable record-field default decline should carry a span"
    );
}

#[test]
fn imported_nominal_record_namespace_private_default_declines_loudly() {
    let error = emit_unchecked_error_for_source_with_files(
        r#"
import model { User }
function main() -> int {
    let u = User { name: "Ada" }
    u.age
}
main()
"#,
        &[
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
const base = 36
export const marker = 0
"#,
            ),
        ],
    );
    assert_eq!(error.code(), "TPZ6PY0001");
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "imported nominal record reference default")
        }
        other => panic!("expected namespace private record default decline, got {other:?}"),
    }
    assert!(
        error.span.is_some(),
        "namespace private record default decline should carry a span"
    );
}

#[test]
fn imported_nominal_record_namespace_nested_default_declines_loudly() {
    let error = emit_unchecked_error_for_source_with_files(
        r#"
import model { User }
function main() -> int {
    let u = User { name: "Ada" }
    u.age
}
main()
"#,
        &[
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
export const inner = { base: 36 }
"#,
            ),
        ],
    );
    assert_eq!(error.code(), "TPZ6PY0001");
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "imported nominal record reference default")
        }
        other => panic!("expected namespace nested record default decline, got {other:?}"),
    }
}

#[test]
fn nominal_record_builtin_namespace_default_declines_loudly() {
    let error = emit_unchecked_error_for_source_with_files(
        r#"
record User { name: string, age: int = Math.abs }
function main() -> int {
    let u = User { name: "Ada" }
    u.age
}
main()
"#,
        &[],
    );
    assert_eq!(error.code(), "TPZ6PY0001");
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "imported nominal record reference default")
        }
        other => panic!("expected builtin namespace record default decline, got {other:?}"),
    }
}

#[test]
fn imported_nominal_record_namespace_optional_default_declines_loudly() {
    let (error, unit) = emit_unchecked_error_and_unit_for_source_with_files(
        r#"
import model { User }
function main() -> int {
    let u = User { name: "Ada" }
    u.age
}
main()
"#,
        &[
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
    assert_eq!(error.code(), "TPZ6PY0001");
    let span = error
        .span
        .expect("namespace optional record default decline should carry a span");
    assert_eq!(text_in_map(&unit.map, span).trim(), "config?.base");
    match &error.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(*what, "imported nominal record reference default")
        }
        other => panic!("expected namespace optional record default decline, got {other:?}"),
    }
}

#[test]
fn top_level_const_collection_does_not_fold_namespace_members() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "main.tpz",
        r#"
import config
const value = config.base
0
"#,
    );
    provider.add_file(
        "config.tpz",
        r#"
export const base = 36
"#,
    );
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_4);
    let entry = unit
        .modules
        .iter()
        .find(|module| module.is_entry)
        .expect("entry module");
    let binding_facts = collect_module_binding_facts(entry, &unit.map);
    let facts = collect_module_default_input_facts(entry, &unit.map, binding_facts);
    assert!(
        !facts
            .const_values
            .own
            .iter()
            .any(|(name, _)| name == "value")
            && facts.const_values.exported.is_empty(),
        "top-level namespace members must not enter ordinary const folding: {:?}",
        facts.const_values.own
    );
}

#[test]
fn generic_nominal_records_emit_like_erased_runtime_records() {
    let generated = emit_source(
        r#"
record Box<T> { value: T }
function main() -> int {
    let b = Box { value: 7 }
    match b {
        case Box { value } => value
    }
}
main()
"#,
    );
    assert!(generated.contains("class _tnr_t_426f78:"), "{generated}");
    assert!(
        generated.contains("tpz_nominal_record(_tnr_t_426f78, \"Box\", "),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_is_nominal_record(__tpz_match, \"Box\")"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("generic nominal record Python gate failed: {e}"));

    let imported = emit_source_with_files(
        r#"
import model { Box as ForeignBox }
function main() -> int {
    let b = ForeignBox { value: 7 }
    b.value
}
main()
"#,
        &[(
            "model.tpz",
            r#"
export record Box<T> { value: T }
"#,
        )],
    );
    assert!(
        imported.contains("class _tnr_t_6d6f64656c___t_426f78:"),
        "{imported}"
    );
    assert!(
        imported.contains("tpz_nominal_record(_tnr_t_6d6f64656c___t_426f78, \"Box\", "),
        "{imported}"
    );
    assert_generated_python_gates(&imported)
        .unwrap_or_else(|e| panic!("imported generic record Python gate failed: {e}"));

    let imported_typed_let = emit_source_with_files(
        r#"
import model { Box as ForeignBox, makeBox }
function main() -> string {
    let b: ForeignBox<int> = makeBox(7)
    let c: ForeignBox<string> = ForeignBox { value: "ok" }
    "{b.value}:{c.value}"
}
main()
"#,
        &[(
            "model.tpz",
            r#"
export record Box<T> { value: T }
export function makeBox(value: int) -> Box<int> {
    Box { value: value }
}
"#,
        )],
    );
    assert!(
        imported_typed_let.contains("(\"nominal_record\", \"Box\",")
            && imported_typed_let.contains("\"int\"")
            && imported_typed_let.contains("\"string\"")
            && imported_typed_let.contains("tpz_type_matches("),
        "{imported_typed_let}"
    );
    assert_generated_python_gates(&imported_typed_let)
        .unwrap_or_else(|e| panic!("imported generic record typed-let Python gate failed: {e}"));
}
