use super::*;

#[test]
fn emits_structural_record_update_through_checked_helper() {
    let generated = emit_source(
        r#"
function main() -> int {
    let r = { x: 1, y: 2 }
    let updated = r { x: 9 }
    updated.x + r.x + updated.y
}
main()
"#,
    );
    assert!(
        generated.contains("__topaz_record_fields__ = ((\"_t_78\", \"x\"), (\"_t_79\", \"y\"))"),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_record_update(_t_72, [(\"_t_78\", \"x\", lambda: 9)], "),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("record update Python gate failed: {e}"));
}

#[test]
fn emits_record_member_assignment_through_update_helper() {
    let generated = emit_source(
        r#"
function main() -> string {
    let mut r = { n: 1, text: "a" }
    r.n = 2
    r.n += loop {
        print("rhs")
        break 3
    }
    r.text ??= mark("skip", "skip")
    r.text = "{r.text}:b"
    "{r.n}:{r.text}"
}
function mark(label: string, value: string) -> string {
    print(label)
    value
}
main()
"#,
    );
    assert!(generated.contains("tpz_record_update("), "{generated}");
    assert!(generated.contains("tpz_member("), "{generated}");
    assert!(generated.contains("tpz_add("), "{generated}");
    assert!(
        generated.contains(" is None or ") && generated.contains(" is TPZ_NULL:"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("record member assignment Python gate failed: {e}"));
}

#[test]
fn nested_record_path_assignment_matches_stage0_rebuild_and_order() {
    let generated = emit_source(
        r#"
function main() -> string {
    let mut record = {
        nested: { n: 1, text: "a", fallback: null },
        keep: 7
    }
    record.nested.n += loop {
        record.nested.text = "b"
        break 3
    }
    record.nested.fallback ??= "x"
    "{record.nested.n}:{record.nested.text}:{record.nested.fallback}:{record.keep}"
}
main()
"#,
    );
    assert_generated_python_ok_string(
        &generated,
        "4:b:x:7",
        "nested record-path assignment rebuild and evaluation-order parity",
    );
}

#[test]
fn static_record_field_assignment_preserves_nested_value_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function fast(x: int) -> int { x }
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
function main() -> int {
    let mut state = {
        nested: { values: [3, 1, 2], callback: sum, cooperative: fast }
    }
    state.nested = {
        values: [4, 2, 3],
        callback: sum,
        cooperative: spin
    }
    state.nested.values.sort()
    let direct = state.nested.callback(5, ...[1, 2])
    let cooperative = concurrent {
        slow: [7].map(state.nested.cooperative)[0]
        fast: 0
    }
    state.nested.values[0] * 100 + direct * 10 + cooperative.slow
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        287,
        "static record field assignment nested receiver callable and cooperative metadata",
    );
}

#[test]
fn static_record_field_assignment_preserves_nested_lambda_call_shape() {
    let generated = emit_checked_alias_source(
        r#"
function add(a: int, b: int) -> int {
    a + b
}
function main() -> int {
    let mut callbacks = { nested: { plus: add } }
    callbacks.nested = { plus: (a, b) => a + b }
    callbacks.nested.plus(b: 3, a: 10)
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        13,
        "static record field assignment nested lambda call shape",
    );
}

#[test]
fn static_nested_record_path_assignment_preserves_leaf_subtree_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function fast(x: int) -> int { x }
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
function main() -> int {
    let mut state = {
        outer: {
            nested: { values: [3, 1, 2], callback: sum, cooperative: fast },
            preserved: { values: [9, 8] }
        },
        outside: { callback: sum }
    }
    state.outer.nested = {
        values: [4, 2, 3],
        callback: sum,
        cooperative: spin
    }
    state.outer.nested.values.sort()
    state.outer.preserved.values.sort()
    let direct = state.outer.nested.callback(5, ...[1, 2])
    let cooperative = concurrent {
        slow: [7].map(state.outer.nested.cooperative)[0]
        fast: 0
    }
    let outside = state.outside.callback(1, ...[2])
    state.outer.nested.values[0] * 1000 + state.outer.preserved.values[0] * 100 + direct * 10 + cooperative.slow + outside
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        2890,
        "static nested record path assignment leaf and sibling metadata",
    );
}

#[test]
fn index_rooted_record_cell_path_assignment_matches_stage0() {
    let generated = emit_source(
        r#"
function main() -> string {
    let mut rows = [{
        nested: { n: 1, text: "a", fallback: null },
        keep: 7
    }]
    rows[0].nested.n += loop {
        rows[0].nested.text = "b"
        break 3
    }
    rows[0].nested.fallback ??= "x"
    "{rows[0].nested.n}:{rows[0].nested.text}:{rows[0].nested.fallback}:{rows[0].keep}"
}
main()
"#,
    );
    assert_generated_python_ok_string(
        &generated,
        "4:b:x:7",
        "index-rooted record cell-path assignment parity",
    );
}

#[test]
fn static_index_rooted_record_path_assignment_preserves_element_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function fast(x: int) -> int { x }
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
function main() -> int {
    let mut rows = [
        {
            nested: { values: [3, 1, 2], callback: sum, cooperative: fast },
            preserved: { values: [9, 8] }
        },
        {
            nested: { values: [3, 2, 1], callback: sum, cooperative: fast },
            preserved: { values: [7, 6] }
        }
    ]
    rows[0].nested = {
        values: [4, 2, 3],
        callback: sum,
        cooperative: spin
    }
    rows[0].nested.values.sort()
    rows[0].preserved.values.sort()
    rows[1].nested.values.sort()
    let direct = rows[0].nested.callback(5, ...[1, 2])
    let cooperative = concurrent {
        slow: [7].map(rows[0].nested.cooperative)[0]
        fast: 0
    }
    let sibling = rows[1].nested.callback(1, ...[2])
    rows[0].nested.values[0] * 1000 + rows[0].preserved.values[0] * 100 + direct * 10 + cooperative.slow + sibling + rows[1].nested.values[0]
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        2891,
        "static index-rooted record path assignment element and sibling metadata",
    );
}

#[test]
fn contextually_typed_lambda_array_record_member_shape_is_available() {
    let generated = emit_checked_alias_source(
        r#"
function main() -> int {
    let firstSorted: (Array<{ values: Array<int> }>) -> int = rows => rows[0].values.sorted()[0]
    firstSorted([{ values: [3, 1, 2] }])
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        1,
        "contextually typed lambda Array-record member receiver shape",
    );
}

#[test]
fn array_option_and_result_record_elements_preserve_wrapper_metadata() {
    let generated = emit_checked_alias_source(
        r#"
type Payload = {
    values: Array<int>,
    nested: { callback: (int, ...int) -> int }
}
function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function scoreOption(values: Array<Option<Payload>>) -> int {
    match values[0] {
        case Some(payload) => payload.values.sorted()[0] * 100 + payload.nested.callback(1, ...[2, 3])
        case None => 0
    }
}
function scoreResult(values: Array<Result<Payload, string>>) -> int {
    match values[0] {
        case Ok(payload) => payload.values.sorted()[0] * 100 + payload.nested.callback(1, ...[2, 3])
        case Err(_) => 0
    }
}
function main() -> int {
    let payload = { values: [4, 2, 3], nested: { callback: sum } }
    scoreOption([Some(payload)]) * 1000 + scoreResult([Ok(payload)])
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        206206,
        "Array Option/Result record element wrapper metadata",
    );
}

#[test]
fn array_map_record_elements_preserve_wrapper_metadata() {
    let generated = emit_checked_alias_source(
        r#"
type Payload = {
    values: Array<int>,
    nested: { callback: (int, ...int) -> int }
}
function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function score(values: Array<Map<string, Payload>>) -> int {
    match values[0].get("payload") {
        case Some(payload) => payload.values.sorted()[0] * 100 + payload.nested.callback(1, ...[2, 3])
        case None => 0
    }
}
function main() -> int {
    let payload = { values: [4, 2, 3], nested: { callback: sum } }
    score([map { "payload": payload }])
}
main()
"#,
    );
    assert_generated_python_ok_int(&generated, 206, "Array Map record element wrapper metadata");
}

#[test]
fn unannotated_array_map_elements_preserve_static_slot_value_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function add(a: int, b: int) -> int { a + b }
function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}
function main() -> int {
    let payload = { values: [4, 2, 3], nested: { callback: sum } }
    let payloadMaps = [map { "payload": payload }]
    let callbackMaps = [map { "callback": add }]
    let payloadScore = match payloadMaps[0].get("payload") {
        case Some(found) => found.values.sorted()[0] * 100 + found.nested.callback(1, ...[2, 3])
        case None => 0
    }
    let callbackScore = match callbackMaps[0].get("callback") {
        case Some(callback) => callback(b: 3, a: 10)
        case None => 0
    }
    payloadScore * 1000 + callbackScore
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        206013,
        "unannotated Array Map static-slot record and callable metadata",
    );
}

#[test]
fn unannotated_map_get_preserves_static_key_value_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function add(a: int, b: int) -> int { a + b }
function inc(x: int) -> int { x + 1 }
function main() -> int {
    let arrayMap = map { "value": [3, 1, 2] }
    let wrappedMap = map { "value": Some([4, 5]) }
    let callbackMap = map { "value": add }
    let arrayScore = match arrayMap.get("value") {
        case Some(xs) => xs.sorted()[0]
        case None => 0
    }
    let wrapped = match wrappedMap.get("value") {
        case Some(maybe) => maybe?.map(inc)
        case None => None
    }
    let wrappedScore = match wrapped {
        case Some(xs) => xs[0]
        case None => 0
    }
    let callbackScore = match callbackMap.get("value") {
        case Some(callback) => callback(b: 3, a: 10)
        case None => 0
    }
    arrayScore * 100 + wrappedScore * 10 + callbackScore
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        163,
        "unannotated Map.get static-key receiver callable and wrapped metadata",
    );
}

#[test]
fn dynamic_map_get_preserves_homogeneous_value_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function add(a: int, b: int) -> int { a + b }
function inc(x: int) -> int { x + 1 }
function main() -> int {
    let arrayMap = map { "left": [3, 1, 2], "right": [6, 4, 5] }
    let callbackMap = map { "left": add, "right": add }
    let wrappedMaps = [map { "left": Some([4, 5]), "right": Some([6, 7]) }]
    let arrayKey = "right"
    let callbackKey = "left"
    let wrappedKey = "right"
    let arrayScore = match arrayMap.get(arrayKey) {
        case Some(xs) => xs.sorted()[0]
        case None => 0
    }
    let callbackScore = match callbackMap.get(callbackKey) {
        case Some(callback) => callback(b: 3, a: 10)
        case None => 0
    }
    let wrapped = match wrappedMaps[0].get(wrappedKey) {
        case Some(maybe) => maybe?.map(inc)
        case None => None
    }
    let wrappedScore = match wrapped {
        case Some(xs) => xs[0]
        case None => 0
    }
    arrayScore * 100 + callbackScore * 10 + wrappedScore
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        537,
        "dynamic Map.get homogeneous receiver callable and wrapped metadata",
    );
}

#[test]
fn dynamic_map_get_declines_incomplete_key_metadata() {
    let error = emit_error_for_source(
        r#"
function left(a: int, b: int = 2) -> int { a + b }
function right(x: int, y: int = 2) -> int { x + y }
function main() -> int {
    let dynamicKey = "left"
    let callbacks = map { dynamicKey: left, "right": right }
    let lookupKey = "left"
    match callbacks.get(lookupKey) {
        case Some(callback) => callback(a: 5)
        case None => 0
    }
}
main()
"#,
    );
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => assert_eq!(what, "call target"),
        other => panic!("expected unsupported call target, got {other:?}"),
    }
}

#[test]
fn static_map_insert_refreshes_addressed_value_metadata() {
    let generated = emit_checked_alias_source_with_files(
        r#"
import util

function main() -> int {
    let mut callbacks = util.callbacks
    callbacks.insert(v: util.right, k: "left")
    let key = "left"
    match callbacks.get(key) {
        case Some(callback) => callback(x: 5)
        case None => 0
    }
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function left(a: int, b: int = 2) -> int { a + b }
export function right(x: int, y: int = 2) -> int { x + y }
export let callbacks = map { "left": left }
"#,
        )],
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "static Map.insert addressed callable metadata refresh",
    );
}

#[test]
fn dynamic_map_insert_invalidates_observed_value_metadata() {
    let error = emit_error_for_source_with_files(
        r#"
import util

function main() -> int {
    let mut callbacks = util.callbacks
    let key = "left"
    callbacks.insert(key, util.right)
    match callbacks.get("left") {
        case Some(callback) => callback(a: 5)
        case None => 0
    }
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function left(a: int, b: int = 2) -> int { a + b }
export function right(x: int, y: int = 2) -> int { x + y }
export let callbacks = map { "left": left }
"#,
        )],
    );
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => assert_eq!(what, "call target"),
        other => panic!("expected unsupported call target, got {other:?}"),
    }
}

#[test]
fn map_parameter_mutation_invalidates_argument_value_metadata() {
    let error = emit_error_for_source_with_files(
        r#"
import util

function replace(
    items: Map<string, (int, int) -> int>,
    next: (int, int) -> int,
) -> unit {
    let mut local = items
    local.insert("left", next)
}

function main() -> int {
    let mut callbacks = util.callbacks
    replace(callbacks, util.right)
    match callbacks.get("left") {
        case Some(callback) => callback(a: 5)
        case None => 0
    }
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function left(a: int, b: int = 2) -> int { a + b }
export function right(x: int, y: int = 2) -> int { x + y }
export let callbacks = map { "left": left }
"#,
        )],
    );
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => assert_eq!(what, "call target"),
        other => panic!("expected unsupported call target, got {other:?}"),
    }
}

#[test]
fn static_map_insert_refreshes_shared_alias_metadata_in_expression_order() {
    let generated = emit_checked_alias_source_with_files(
        r#"
import util

function main() -> int {
    let mut callbacks = util.callbacks
    let mut alias = callbacks
    let product = {
        done: alias.insert("left", util.right),
        value: match callbacks.get("left") {
            case Some(callback) => callback(x: 5)
            case None => 0
        },
    }
    product.value
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function left(a: int, b: int = 2) -> int { a + b }
export function right(x: int, y: int = 2) -> int { x + y }
export let callbacks = map { "left": left }
"#,
        )],
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "static Map.insert shared alias ordered metadata refresh",
    );
}

#[test]
fn static_map_insert_refreshes_receiver_and_wrapped_value_metadata() {
    let generated = emit_checked_alias_source_with_files(
        r#"
import util

function inc(x: int) -> int { x + 1 }
function main() -> int {
    let mut arrays = util.arrays
    arrays.insert("value", [9, 7, 8])
    let arrayScore = match arrays.get("value") {
        case Some(xs) => xs.sorted()[0]
        case None => 0
    }

    let mut wrapped = util.wrapped
    wrapped.insert("value", Some([6, 4, 5]))
    let wrappedScore = match wrapped.get("value") {
        case Some(maybe) => match maybe?.map(inc) {
            case Some(xs) => xs[0]
            case None => 0
        }
        case None => 0
    }
    arrayScore * 100 + wrappedScore
}
main()
"#,
        &[(
            "util.tpz",
            r#"
export let arrays = map { "value": [3, 1, 2] }
export let wrapped = map { "value": Some([3, 1, 2]) }
"#,
        )],
    );
    assert_generated_python_ok_int(
        &generated,
        707,
        "static Map.insert receiver and wrapped metadata refresh",
    );
}

#[test]
fn static_map_insert_restores_metadata_after_clear() {
    let generated = emit_checked_alias_source_with_files(
        r#"
import util

function main() -> int {
    let mut callbacks = util.callbacks
    callbacks.clear()
    callbacks.insert("right", util.right)
    let key = "right"
    match callbacks.get(key) {
        case Some(callback) => callback(x: 5)
        case None => 0
    }
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function left(a: int, b: int = 2) -> int { a + b }
export function right(x: int, y: int = 2) -> int { x + y }
export let callbacks = map { "left": left }
"#,
        )],
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "static Map.insert restores metadata after clear",
    );
}

#[test]
fn static_map_update_uses_present_callback_metadata() {
    let generated = emit_checked_alias_source_with_files(
        r#"
import util

function main() -> int {
    let mut callbacks = util.callbacks
    callbacks.update(f: (_current) => util.right, initial: util.left, k: "left")
    let key = "left"
    match callbacks.get(key) {
        case Some(callback) => callback(x: 5)
        case None => 0
    }
}
main()
"#,
        &[(
            "util.tpz",
            r#"
export function left(a: int, b: int = 2) -> int { a + b }
export function right(x: int, y: int = 2) -> int { x + y }
export let callbacks = map { "left": left }
"#,
        )],
    );
    assert_generated_python_ok_int(&generated, 7, "static Map.update present callback metadata");
}

#[test]
fn static_map_update_uses_absent_initial_metadata() {
    let generated = emit_checked_alias_source_with_files(
        r#"
import util

function main() -> int {
    let mut callbacks = util.callbacks
    callbacks.update("right", util.right, (_current) => util.left)
    match callbacks.get("right") {
        case Some(callback) => callback(x: 5)
        case None => 0
    }
}
main()
"#,
        &[(
            "util.tpz",
            r#"
export function left(a: int, b: int = 2) -> int { a + b }
export function right(x: int, y: int = 2) -> int { x + y }
export let callbacks = map { "left": left }
"#,
        )],
    );
    assert_generated_python_ok_int(&generated, 7, "static Map.update absent initial metadata");
}

#[test]
fn static_map_update_preserves_unaffected_metadata_when_callback_result_is_unknown() {
    let generated = emit_checked_alias_source_with_files(
        r#"
import util

function main() -> int {
    let mut callbacks = util.callbacks
    callbacks.update("left", util.left, (current) => current)
    match callbacks.get("right") {
        case Some(callback) => callback(x: 5)
        case None => 0
    }
}
main()
"#,
        &[(
            "util.tpz",
            r#"
export function left(a: int, b: int = 2) -> int { a + b }
function right(x: int, y: int = 2) -> int { x + y }
export let callbacks = map { "left": left, "right": right }
"#,
        )],
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "static Map.update preserves unaffected metadata",
    );
}

#[test]
fn delayed_lambda_collection_mutation_does_not_apply_at_creation() {
    let generated = emit_checked_alias_source_with_files(
        r#"
import util

function main() -> int {
    let mut callbacks = util.callbacks
    let _delayed = () => callbacks.clear()
    match callbacks.get("left") {
        case Some(callback) => callback(a: 5)
        case None => 0
    }
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function left(a: int, b: int = 2) -> int { a + b }
export let callbacks = map { "left": left }
"#,
        )],
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "delayed lambda collection mutation remains delayed",
    );
}

#[test]
fn local_mutable_map_aliases_share_precise_mutation_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function left(a: int, b: int = 2) -> int { a + b }
function right(x: int, y: int = 2) -> int { x + y }
function main() -> int {
    let mut callbacks = map { "left": left, "right": left }
    let mut alias = callbacks
    alias.insert("right", right)
    callbacks.remove("left")
    let key = "right"
    match alias.get(key) {
        case Some(callback) => callback(x: 5)
        case None => 0
    }
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "local mutable Map aliases share precise mutation metadata",
    );
}

#[test]
fn local_mutable_array_alias_preserves_static_slot_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function left(a: int, b: int = 2) -> int { a + b }
function main() -> int {
    let mut callbacks = [left]
    let alias = callbacks
    alias[0](a: 5)
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "local mutable Array alias static slot metadata",
    );
}

#[test]
fn local_mutable_array_alias_shares_static_index_assignment_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function left(a: int, b: int = 2) -> int { a + b }
function right(x: int, y: int = 2) -> int { x + y }
function main() -> int {
    let mut callbacks = [left]
    let mut alias = callbacks
    alias[0] = right
    callbacks[0](x: 5)
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "local mutable Array alias static index assignment metadata",
    );
}

#[test]
fn local_mutable_array_aliases_share_precise_mutator_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function left(a: int, b: int = 2) -> int { a + b }
function right(x: int, y: int = 2) -> int { x + y }
function main() -> int {
    let mut pushed = [left]
    let mut pushedAlias = pushed
    pushedAlias.push(x: right)
    let pushedResult = pushed[1](x: 5)

    let mut inserted = [left]
    let mut insertedAlias = inserted
    insertedAlias.insert(value: right, index: 0)
    let insertedResult = inserted[0](x: 5)

    let mut popped = [left, right]
    let mut poppedAlias = popped
    poppedAlias.pop()
    let poppedResult = popped[0](a: 5)

    let mut removed = [left, right]
    let mut removedAlias = removed
    removedAlias.removeAt(0)
    let removedResult = removed[0](x: 5)

    let mut cleared = [left]
    let mut clearedAlias = cleared
    clearedAlias.clear()
    clearedAlias.push(right)
    let clearedResult = cleared[0](x: 5)

    let mut reversed = [left, right]
    let mut reversedAlias = reversed
    reversedAlias.reverse()
    let reversedResult = reversed[0](x: 5)

    pushedResult + insertedResult + poppedResult + removedResult + clearedResult + reversedResult
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        42,
        "local mutable Array aliases share precise mutator metadata",
    );
}

#[test]
fn local_mutable_array_reordering_preserves_homogeneous_callable_abi() {
    let sorted = emit_checked_alias_source(
        r#"
function left(a: int, b: int = 2) -> int { a + b }
function right(a: int, b: int = 2) -> int { a + b }
function main() -> int {
    let mut callbacks = [left, right]
    let alias = callbacks
    callbacks.sortBy((callback) => 0)
    let indices = [0]
    let i = indices[0]
    alias[i](a: 5)
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &sorted,
        7,
        "local mutable Array sortBy preserves homogeneous callable ABI",
    );

    let retained = emit_checked_alias_source(
        r#"
function left(a: int, b: int = 2) -> int { a + b }
function right(a: int, b: int = 2) -> int { a + b }
function main() -> int {
    let mut callbacks = [left, right]
    let alias = callbacks
    callbacks.retain((callback) => true)
    let indices = [0]
    let i = indices[0]
    alias[i](a: 5)
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &retained,
        7,
        "local mutable Array retain preserves homogeneous callable ABI",
    );
}

#[test]
fn static_map_remove_preserves_unaffected_alias_metadata_in_expression_order() {
    let generated = emit_checked_alias_source_with_files(
        r#"
import util

function main() -> int {
    let mut callbacks = util.callbacks
    let mut alias = callbacks
    let key = "right"
    let product = {
        done: alias.remove(k: "left"),
        value: match callbacks.get(key) {
            case Some(callback) => callback(x: 5)
            case None => 0
        },
    }
    product.value
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function left(a: int, b: int = 2) -> int { a + b }
function right(x: int, y: int = 2) -> int { x + y }
export let callbacks = map { "left": left, "right": right }
"#,
        )],
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "static Map.remove preserves unaffected alias metadata in expression order",
    );
}

#[test]
fn static_map_remove_drops_addressed_value_metadata() {
    let error = emit_error_for_source_with_files(
        r#"
import util

function main() -> int {
    let mut callbacks = util.callbacks
    callbacks.remove("left")
    match callbacks.get("left") {
        case Some(callback) => callback(a: 5)
        case None => 0
    }
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function left(a: int, b: int = 2) -> int { a + b }
export let callbacks = map { "left": left }
"#,
        )],
    );
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => assert_eq!(what, "call target"),
        other => panic!("expected unsupported call target, got {other:?}"),
    }
}

#[test]
fn dynamic_map_remove_preserves_surviving_value_metadata() {
    let generated = emit_checked_alias_source_with_files(
        r#"
import util

function main() -> int {
    let mut callbacks = util.callbacks
    let removed = "left"
    callbacks.remove(removed)
    let selected = "right"
    match callbacks.get(selected) {
        case Some(callback) => callback(x: 5)
        case None => 0
    }
}
main()
"#,
        &[(
            "util.tpz",
            r#"
function add(x: int, y: int = 2) -> int { x + y }
export let callbacks = map { "left": add, "right": add }
"#,
        )],
    );
    assert_generated_python_ok_int(
        &generated,
        7,
        "dynamic Map.remove preserves surviving homogeneous value metadata",
    );
}

#[test]
fn unannotated_nested_array_and_option_elements_preserve_slot_shapes() {
    let generated = emit_checked_alias_source(
        r#"
function inc(x: int) -> int { x + 1 }
function main() -> int {
    let nested = [[3, 1, 2]]
    let maybe = [Some([6, 4, 5])]
    let direct = nested[0].sorted()[0]
    let optional = match maybe[0]?.map(inc) {
        case Some(xs) => xs[1] * 10 + xs[0]
        case None => 0
    }
    direct * 100 + optional
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        157,
        "unannotated nested Array and Option<Array> static-slot shapes",
    );
}

#[test]
fn array_option_result_patterns_preserve_inner_shape_and_callable_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function add(a: int, b: int) -> int { a + b }
function main() -> int {
    let optionArrays = [Some([3, 1, 2])]
    let resultArrays = [Ok([6, 4, 5])]
    let optionCallbacks = [Some(add)]
    let resultCallbacks = [Ok(add)]
    let optionArray = match optionArrays[0] {
        case Some(xs) => xs.sorted()[0]
        case None => 0
    }
    let resultArray = match resultArrays[0] {
        case Ok(xs) => xs.sorted()[0]
        case Err(_) => 0
    }
    let optionCallback = match optionCallbacks[0] {
        case Some(callback) => callback(b: 3, a: 10)
        case None => 0
    }
    let resultCallback = match resultCallbacks[0] {
        case Ok(callback) => callback(b: 3, a: 10)
        case Err(_) => 0
    }
    optionArray * 1000 + resultArray * 100 + optionCallback * 10 + resultCallback
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        1543,
        "Array Option/Result pattern inner receiver and callable metadata",
    );
}

#[test]
fn bound_option_result_patterns_preserve_inner_shape_and_callable_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function add(a: int, b: int) -> int { a + b }
function main() -> int {
    let optionArray = Some([3, 1, 2])
    let resultArray: Result<Array<int>, string> = Ok([6, 4, 5])
    let optionCallback = Some(add)
    let resultCallback: Result<(int, int) -> int, string> = Ok(add)
    let optionArrayScore = match optionArray {
        case Some(xs) => xs.sorted()[0]
        case None => 0
    }
    let resultArrayScore = match resultArray {
        case Ok(xs) => xs.sorted()[0]
        case Err(_) => 0
    }
    let optionCallbackScore = match optionCallback {
        case Some(callback) => callback(b: 3, a: 10)
        case None => 0
    }
    let resultCallbackScore = match resultCallback {
        case Ok(callback) => callback(b: 3, a: 10)
        case Err(_) => 0
    }
    optionArrayScore * 1000 + resultArrayScore * 100 + optionCallbackScore * 10 + resultCallbackScore
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        1543,
        "bound Option/Result pattern inner receiver and callable metadata",
    );
}

#[test]
fn function_return_option_result_patterns_preserve_wrapped_value_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function add(a: int, b: int) -> int { a + b }
function optionArray() { Some([3, 1, 2]) }
function resultArray() -> Result<Array<int>, string> { Ok([6, 4, 5]) }
function optionCallback() { Some(add) }
function resultCallback() -> Result<(int, int) -> int, string> { Ok(add) }
function main() -> int {
    let optionArrayScore = match optionArray() {
        case Some(xs) => xs.sorted()[0]
        case None => 0
    }
    let resultArrayScore = match resultArray() {
        case Ok(xs) => xs.sorted()[0]
        case Err(_) => 0
    }
    let optionCallbackScore = match optionCallback() {
        case Some(callback) => callback(b: 3, a: 10)
        case None => 0
    }
    let resultCallbackScore = match resultCallback() {
        case Ok(callback) => callback(b: 3, a: 10)
        case Err(_) => 0
    }
    optionArrayScore * 1000 + resultArrayScore * 100 + optionCallbackScore * 10 + resultCallbackScore
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        1543,
        "function-returned Option/Result pattern inner receiver and callable metadata",
    );
}

#[test]
fn branch_option_result_patterns_preserve_wrapped_value_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function add(a: int, b: int) -> int { a + b }
function main() -> int {
    let flag = true
    let directOptionArrayScore = match (if flag { Some([3, 1, 2]) } else { Some([3, 1, 2]) }) {
        case Some(xs) => xs.sorted()[0]
        case None => 0
    }
    let boundResultArray: Result<Array<int>, string> = match flag {
        case true => Ok([6, 4, 5])
        case _ => Ok([6, 4, 5])
    }
    let boundResultArrayScore = match boundResultArray {
        case Ok(xs) => xs.sorted()[0]
        case Err(_) => 0
    }
    let optionCallbacks = [if flag { Some(add) } else { Some(add) }]
    let optionCallbackScore = match optionCallbacks[0] {
        case Some(callback) => callback(b: 3, a: 10)
        case None => 0
    }
    let mut resultCallbacks: Array<Result<(int, int) -> int, string>> = [Ok(add)]
    resultCallbacks[0] = match flag {
        case true => Ok(add)
        case _ => Ok(add)
    }
    let resultCallbackScore = match resultCallbacks[0] {
        case Ok(callback) => callback(b: 3, a: 10)
        case Err(_) => 0
    }
    directOptionArrayScore * 1000 + boundResultArrayScore * 100 + optionCallbackScore * 10 + resultCallbackScore
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        1543,
        "branch-joined Option/Result pattern inner receiver and callable metadata",
    );
}

#[test]
fn optional_receiver_uses_shared_option_wrapper_projection() {
    let generated = emit_source_with_files(
        r#"
import util

function inc(x: int) -> int { x + 1 }
function main() -> int {
    let flag = true
    let direct = (if flag { Some([1, 2]) } else { Some([1, 2]) })?.map(inc)
    let namespace = util.values?.map(inc)
    let outer = Some(if flag { Some([5, 6]) } else { Some([5, 6]) })
    let nested = match outer {
        case Some(inner) => inner?.map(inc)
        case None => None
    }
    let directScore = match direct {
        case Some(xs) => xs[0]
        case None => 0
    }
    let namespaceScore = match namespace {
        case Some(xs) => xs[0]
        case None => 0
    }
    let nestedScore = match nested {
        case Some(xs) => xs[0]
        case None => 0
    }
    directScore * 100 + namespaceScore * 10 + nestedScore
}
main()
"#,
        &[("util.tpz", "export let values = Some([3, 4])\n")],
    );
    assert_generated_python_ok_int(
        &generated,
        246,
        "optional receiver shared Option wrapper projection",
    );
}
