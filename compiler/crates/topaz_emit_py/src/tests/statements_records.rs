use super::*;

#[test]
fn typed_mutable_arrays_preserve_declared_element_receiver_shapes() {
    let generated = emit_checked_alias_source(
        r#"
function main() -> int {
    let mut rebound: Array<Array<int>> = [[9]]
    rebound = [[3, 1, 2]]
    rebound[0].sort()
    let reboundValue = rebound[0][0]

    let mut indexed: Array<Array<int>> = [[9]]
    indexed[0] = [1, 2]
    indexed[0].reverse()
    let indexedValue = indexed[0][0]

    let mut optional: Array<Option<Array<int>>> = [None]
    optional = [Some([4, 5])]
    let optionalValue = optional[0]?.length ?? 0

    reboundValue * 100 + indexedValue * 10 + optionalValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        122,
        "typed mutable Array declared element receiver shapes",
    );
}

#[test]
fn namespace_array_indexes_project_declared_element_receiver_shapes() {
    let generated = emit_checked_alias_source_with_files(
        r#"
import util

function main() -> int {
    let first = util.rows[0].sorted()[0]
    let optionalLength = util.optionalRows[0]?.length ?? 0
    first * 10 + optionalLength
}
main()
"#,
        &[(
            "util.tpz",
            r#"
type Rows = Array<Array<int>>
type OptionalRows = Array<Option<Array<int>>>

export let rows: Rows = [[3, 1, 2]]
export let optionalRows: OptionalRows = [Some([4, 5])]
"#,
        )],
    );
    assert_generated_python_ok_int(
        &generated,
        12,
        "namespace Array index declared element receiver projection",
    );
}

#[test]
fn nominal_record_constructors_preserve_field_receiver_shapes() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
record Bucket { items: Array<(int) -> int> }
function makeBucket() -> Bucket {
    Bucket { items: [first, second] }
}
function main() -> int {
    let direct = Bucket { items: [first, second] }
    let mut directItems = direct.items
    directItems.reverse()
    let directValue = [0].map(directItems[0])[0]

    let returned = makeBucket()
    let mut returnedItems = returned.items
    returnedItems.reverse()
    let returnedValue = [0].map(returnedItems[0])[0]

    directValue * 10 + returnedValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        22,
        "nominal record constructor field receiver shapes",
    );
}

#[test]
fn structural_record_updates_preserve_field_receiver_shapes() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function main() -> int {
    let base = {
        items: [first, second],
        kept: [first, second],
        nested: { items: [first, second] },
        label: "base"
    }

    let overridden = base { items: [first, second] }
    let mut overriddenItems = overridden.items
    overriddenItems.reverse()
    let overriddenValue = [0].map(overriddenItems[0])[0]

    let retained = base { label: "updated" }
    let mut retainedItems = retained.kept
    retainedItems.reverse()
    let retainedValue = [0].map(retainedItems[0])[0]

    let mut nestedItems = retained.nested.items
    nestedItems.reverse()
    let nestedValue = [0].map(nestedItems[0])[0]

    overriddenValue * 100 + retainedValue * 10 + nestedValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        222,
        "structural record update field receiver shapes",
    );
}

#[test]
fn record_updates_preserve_callable_field_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function add(a: int, b: int = 2) -> int { a + b }
function multiply(left: int, right: int) -> int { left * right }
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
record BinaryHandler { callback: (int, int) -> int }
record UnaryHandler { callback: (int) -> int }
function main() -> int {
    let base = { callback: add, label: "base" }
    let retained = base { label: "retained" }
    let retainedValue = retained.callback(a: 5)

    let overridden = base { callback: multiply }
    let overriddenValue = overridden.callback(left: 3, right: 4)

    let nominal = BinaryHandler { callback: add }
    let nominalValue = nominal.callback(a: 5)

    let cooperativeBase = { callback: spin, label: "base" }
    let cooperative = cooperativeBase { label: "updated" }
    let cooperativeResult = concurrent {
        slow: [4].map(cooperative.callback)[0]
        fast: 0
    }

    let nominalCooperative = UnaryHandler { callback: spin }
    let nominalCooperativeResult = concurrent {
        slow: [5].map(nominalCooperative.callback)[0]
        fast: 0
    }

    retainedValue * 1000000 + overriddenValue * 10000 + nominalValue * 100 + cooperativeResult.slow * 10 + nominalCooperativeResult.slow
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        7_120_745,
        "record update callable field ABI and cooperative targets",
    );
}

#[test]
fn nominal_record_defaults_preserve_nested_callable_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function add(a: int, b: int = 2) -> int { a + b }
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
record Handler {
    binary: { callback: (int, int) -> int } = { callback: add },
    unary: { callback: (int) -> int } = { callback: spin }
}
function main() -> int {
    let handler = Handler {}
    let direct = handler.binary.callback(a: 5)
    let cooperative = concurrent {
        slow: [5].map(handler.unary.callback)[0]
        fast: 0
    }
    direct * 10 + cooperative.slow
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        75,
        "nominal record default nested callable ABI and cooperative target",
    );
}

#[test]
fn imported_nominal_record_defaults_preserve_defining_callable_metadata() {
    let selected = r#"
function addSelected(a: int, b: int = 2) -> int { a + b }
function spinSelected(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
export record SelectedHandler {
    binary: { callback: (int, int) -> int } = { callback: addSelected },
    unary: { callback: (int) -> int } = { callback: spinSelected }
}
"#;
    let namespaced = r#"
function addNamespaced(a: int, b: int = 3) -> int { a + b }
function spinNamespaced(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
export record NamespacedHandler {
    binary: { callback: (int, int) -> int } = { callback: addNamespaced },
    unary: { callback: (int) -> int } = { callback: spinNamespaced }
}
"#;
    let generated = emit_checked_alias_source_with_files(
        r#"
import selected { SelectedHandler }
import namespaced { NamespacedHandler }
function main() -> int {
    let selectedHandler = SelectedHandler {}
    let selectedDirect = selectedHandler.binary.callback(a: 5)
    let selectedCooperative = concurrent {
        slow: [5].map(selectedHandler.unary.callback)[0]
        fast: 0
    }
    let selectedScore = selectedDirect * 10 + selectedCooperative.slow

    let namespacedHandler = NamespacedHandler {}
    let namespacedDirect = namespacedHandler.binary.callback(a: 5)
    let namespacedCooperative = concurrent {
        slow: [6].map(namespacedHandler.unary.callback)[0]
        fast: 0
    }
    let namespacedScore = namespacedDirect * 10 + namespacedCooperative.slow

    selectedScore * 100 + namespacedScore
}
main()
"#,
        &[("selected.tpz", selected), ("namespaced.tpz", namespaced)],
    );
    assert_generated_python_ok_int(
        &generated,
        7586,
        "two imported nominal defaults preserve defining callable metadata",
    );
}

#[test]
fn imported_nominal_record_default_helper_calls_use_stable_return_metadata() {
    let handlers = r#"
function add(a: int, b: int = 2) -> int { a + b }
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
function makeBinary() {
    { callback: add }
}
function makeUnary() {
    { callback: spin }
}
export record Handler {
    binary: { callback: (int, int) -> int } = makeBinary(),
    unary: { callback: (int) -> int } = makeUnary()
}
"#;
    let generated = emit_checked_alias_source_with_files(
        r#"
import handlers { Handler }
function main() -> int {
    let handler = Handler {}
    let direct = handler.binary.callback(a: 5)
    let cooperative = concurrent {
        slow: [5].map(handler.unary.callback)[0]
        fast: 0
    }
    direct * 10 + cooperative.slow
}
main()
"#,
        &[("handlers.tpz", handlers)],
    );
    assert_generated_python_ok_int(
        &generated,
        75,
        "imported nominal default helper call return metadata",
    );
}

#[test]
fn function_returns_preserve_record_cooperative_callback_targets() {
    let selected = r#"
function spinSelected(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
export function makeSelected() {
    { callback: spinSelected }
}
"#;
    let namespaced = r#"
function spinNamespaced(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
export function makeNamespaced() {
    { callback: spinNamespaced }
}
"#;
    let generated = emit_checked_alias_source_with_files(
        r#"
import selected { makeSelected }
import namespaced
function spinLocal(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
function makeLocal() {
    { callback: spinLocal }
}
function main() -> int {
    let localResult = concurrent {
        slow: [3].map(makeLocal().callback)[0]
        fast: 0
    }
    let selectedResult = concurrent {
        slow: [4].map(makeSelected().callback)[0]
        fast: 0
    }
    let namespacedResult = concurrent {
        slow: [5].map(namespaced.makeNamespaced().callback)[0]
        fast: 0
    }
    localResult.slow * 100 + selectedResult.slow * 10 + namespacedResult.slow
}
main()
"#,
        &[("selected.tpz", selected), ("namespaced.tpz", namespaced)],
    );
    assert_generated_python_ok_int(
        &generated,
        345,
        "local selected and namespace function return record cooperative callbacks",
    );
}

#[test]
fn transitive_function_returns_preserve_record_cooperative_callback_targets() {
    let selected = r#"
function spinSelected(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
function makeSelected() {
    { callback: spinSelected }
}
export function wrapSelected() {
    makeSelected()
}
"#;
    let namespaced = r#"
function spinNamespaced(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
function makeNamespaced() {
    { callback: spinNamespaced }
}
export function wrapNamespaced() {
    makeNamespaced()
}
"#;
    let generated = emit_checked_alias_source_with_files(
        r#"
import selected { wrapSelected }
import namespaced
function spinLocal(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
function makeLocal() {
    { callback: spinLocal }
}
function wrapLocal() {
    makeLocal()
}
function main() -> int {
    let localResult = concurrent {
        slow: [6].map(wrapLocal().callback)[0]
        fast: 0
    }
    let selectedResult = concurrent {
        slow: [7].map(wrapSelected().callback)[0]
        fast: 0
    }
    let namespacedResult = concurrent {
        slow: [8].map(namespaced.wrapNamespaced().callback)[0]
        fast: 0
    }
    localResult.slow * 100 + selectedResult.slow * 10 + namespacedResult.slow
}
main()
"#,
        &[("selected.tpz", selected), ("namespaced.tpz", namespaced)],
    );
    assert_generated_python_ok_int(
        &generated,
        678,
        "transitive local selected and namespace function return record cooperative callbacks",
    );
}

#[test]
fn function_return_record_metadata_is_declaration_order_independent() {
    let selected = r#"
export function wrapSelected() -> { callback: (int) -> int, values: Array<int> } {
    makeSelected()
}
function spinSelected(value: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    value
}
function makeSelected() -> { callback: (int) -> int, values: Array<int> } {
    { callback: spinSelected, values: [0, 7] }
}
"#;
    let generated = emit_checked_alias_source_with_files(
        r#"
import selected { wrapSelected }
function wrapLocal() -> { callback: (int) -> int, values: Array<int> } {
    makeLocal()
}
function spinLocal(value: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    value
}
function makeLocal() -> { callback: (int) -> int, values: Array<int> } {
    { callback: spinLocal, values: [0, 6] }
}
function main() -> int {
    let local = wrapLocal()
    let mut localValues = local.values
    localValues.reverse()
    let localConcurrent = concurrent {
        slow: [6].map(local.callback)[0]
        fast: 0
    }
    let localScore = localValues[0] * 10 + localConcurrent.slow

    let selectedRecord = wrapSelected()
    let mut selectedValues = selectedRecord.values
    selectedValues.reverse()
    let selectedConcurrent = concurrent {
        slow: [7].map(selectedRecord.callback)[0]
        fast: 0
    }
    let selectedScore = selectedValues[0] * 10 + selectedConcurrent.slow

    localScore * 100 + selectedScore
}
main()
"#,
        &[("selected.tpz", selected)],
    );
    assert_generated_python_ok_int(
        &generated,
        6677,
        "forward local and selected function return record metadata",
    );
}

#[test]
fn namespace_function_calls_preserve_return_record_metadata() {
    let namespaced = r#"
export function wrapNamespaced() -> { callback: (int) -> int, values: Array<int> } {
    makeNamespaced()
}
function spinNamespaced(value: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    value
}
function makeNamespaced() -> { callback: (int) -> int, values: Array<int> } {
    { callback: spinNamespaced, values: [0, 8] }
}
"#;
    let generated = emit_checked_alias_source_with_files(
        r#"
import namespaced
function main() -> int {
    let record = namespaced.wrapNamespaced()
    let mut values = record.values
    values.reverse()
    let direct = record.callback(8)
    let cooperative = concurrent {
        slow: [8].map(record.callback)[0]
        fast: 0
    }
    values[0] * 100 + direct * 10 + cooperative.slow
}
main()
"#,
        &[("namespaced.tpz", namespaced)],
    );
    assert_generated_python_ok_int(
        &generated,
        888,
        "namespace function return record receiver callable and cooperative metadata",
    );
}

#[test]
fn option_record_function_returns_preserve_field_receiver_shapes() {
    let selected = r#"
export function makeSelected() {
    Some({ values: [4, 3, 2, 1] })
}
"#;
    let namespaced = r#"
export function makeNamespaced() {
    Some({ values: [5, 4, 3, 2, 1] })
}
"#;
    let generated = emit_checked_alias_source_with_files(
        r#"
import selected { makeSelected }
import namespaced
function makeLocal() {
    Some({ values: [3, 2, 1] })
}
function main() -> int {
    let localSorted = makeLocal()?.values?.sorted()
    let localLength = localSorted?.length ?? 0

    let selectedSorted = makeSelected()?.values?.sorted()
    let selectedLength = selectedSorted?.length ?? 0

    let namespacedSorted = namespaced.makeNamespaced()?.values?.sorted()
    let namespacedLength = namespacedSorted?.length ?? 0

    localLength * 100 + selectedLength * 10 + namespacedLength
}
main()
"#,
        &[("selected.tpz", selected), ("namespaced.tpz", namespaced)],
    );
    assert_generated_python_ok_int(
        &generated,
        345,
        "local selected and namespace option record return receiver shapes",
    );
}

#[test]
fn option_record_value_bindings_preserve_field_receiver_shapes() {
    let selected = r#"
function makeSelected() {
    Some({ values: [4, 3, 2, 1] })
}
export let selectedValue = makeSelected()
"#;
    let namespaced = r#"
function makeNamespaced() {
    Some({ values: [5, 4, 3, 2, 1] })
}
export let namespacedValue = makeNamespaced()
"#;
    let generated = emit_checked_alias_source_with_files(
        r#"
import selected { selectedValue }
import namespaced
function makeLocal() {
    Some({ values: [3, 2, 1] })
}
function main() -> int {
    let localValue = makeLocal()
    let localSorted = localValue?.values?.sorted()
    let localLength = localSorted?.length ?? 0

    let selectedSorted = selectedValue?.values?.sorted()
    let selectedLength = selectedSorted?.length ?? 0

    let namespacedSorted = namespaced.namespacedValue?.values?.sorted()
    let namespacedLength = namespacedSorted?.length ?? 0

    localLength * 100 + selectedLength * 10 + namespacedLength
}
main()
"#,
        &[("selected.tpz", selected), ("namespaced.tpz", namespaced)],
    );
    assert_generated_python_ok_int(
        &generated,
        345,
        "local selected and namespace option record value receiver shapes",
    );
}

#[test]
fn typed_mutable_option_record_bindings_preserve_declared_field_receiver_shapes() {
    let generated = emit_checked_alias_source(
        r#"
type Payload = { values: Array<int> }
type MaybePayload = Option<Payload>

function main() -> int {
    let mut direct: Option<{ values: Array<int> }> = Some({ values: [1] })
    if true {
        direct = Some({ values: [3, 2, 1] })
    } else {
        direct = None
    }
    let directSorted = direct?.values?.sorted()
    let directLength = directSorted?.length ?? 0

    let mut aliased: MaybePayload = Some({ values: [1] })
    aliased = None
    aliased = Some({ values: [4, 3, 2, 1] })
    let aliasedSorted = aliased?.values?.sorted()
    let aliasedLength = aliasedSorted?.length ?? 0

    directLength * 10 + aliasedLength
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        34,
        "typed mutable direct and aliased option record receiver shapes",
    );
}

#[test]
fn declared_option_record_types_preserve_field_callable_abi() {
    let generated = emit_checked_alias_source(
        r#"
type Callbacks = { total: (int, ...int) -> int }
type MaybeCallbacks = Option<Callbacks>

function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}

function maybeCallbacks(enabled: bool) -> Option<{ total: (int, ...int) -> int }> {
    if enabled {
        Some({ total: sum })
    } else {
        None
    }
}

function main() -> int {
    let mut direct: Option<{ total: (int, ...int) -> int }> = Some({ total: sum })
    if true {
        direct = Some({ total: sum })
    } else {
        direct = None
    }
    let directValue = direct?.total(3, ...[1, 2]) ?? 0

    let mut aliased: MaybeCallbacks = Some({ total: sum })
    aliased = None
    aliased = Some({ total: sum })
    let aliasedValue = aliased?.total(4, ...[1, 2]) ?? 0

    let returnedValue = maybeCallbacks(true)?.total(3, ...[1, 2]) ?? 0
    directValue * 100 + aliasedValue * 10 + returnedValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        676,
        "typed mutable and declared return option record callable ABI",
    );
}

#[test]
fn declared_record_return_types_preserve_statementful_field_metadata() {
    let selected = r#"
function addSelected(value: int, delta: int) -> int {
    value + delta
}
export function makeSelected() -> { callback: (int, int) -> int, values: Array<int> } {
    let seed = 7
    { callback: addSelected, values: [0, seed] }
}
"#;
    let generated = emit_checked_alias_source_with_files(
        r#"
import selected { makeSelected }
function addLocal(value: int, delta: int) -> int {
    value + delta
}
function makeLocal() -> { callback: (int, int) -> int, values: Array<int> } {
    let seed = 6
    { callback: addLocal, values: [0, seed] }
}
function main() -> int {
    let local = makeLocal()
    let mut localValues = local.values
    localValues.reverse()
    let localScore = localValues[0] * 10 + local.callback(6, 0)

    let selectedRecord = makeSelected()
    let mut selectedValues = selectedRecord.values
    selectedValues.reverse()
    let selectedScore = selectedValues[0] * 10 + selectedRecord.callback(7, 0)

    localScore * 100 + selectedScore
}
main()
"#,
        &[("selected.tpz", selected)],
    );
    assert_generated_python_ok_int(
        &generated,
        6677,
        "declared record return field receiver and callable metadata",
    );
}

#[test]
fn declared_nested_record_return_types_preserve_descendant_field_metadata() {
    let selected = r#"
export type SelectedBundle = {
    nested: { callback: (int, int) -> int, values: Array<int> }
}
function addSelected(value: int, delta: int) -> int {
    value + delta
}
export function makeSelected() -> SelectedBundle {
    let seed = 7
    { nested: { callback: addSelected, values: [0, seed] } }
}
"#;
    let generated = emit_checked_alias_source_with_files(
        r#"
import selected { makeSelected }
function addLocal(value: int, delta: int) -> int {
    value + delta
}
function makeLocal() -> { nested: { callback: (int, int) -> int, values: Array<int> } } {
    let seed = 6
    { nested: { callback: addLocal, values: [0, seed] } }
}
function main() -> int {
    let local = makeLocal()
    let mut localValues = local.nested.values
    localValues.reverse()
    let localScore = localValues[0] * 10 + local.nested.callback(6, 0)

    let selectedRecord = makeSelected()
    let mut selectedValues = selectedRecord.nested.values
    selectedValues.reverse()
    let selectedScore = selectedValues[0] * 10 + selectedRecord.nested.callback(7, 0)

    localScore * 100 + selectedScore
}
main()
"#,
        &[("selected.tpz", selected)],
    );
    assert_generated_python_ok_int(
        &generated,
        6677,
        "declared nested record return descendant receiver and callable metadata",
    );
}
