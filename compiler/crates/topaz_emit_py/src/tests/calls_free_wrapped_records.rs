use super::*;

#[test]
fn typed_maps_preserve_declared_record_value_metadata_through_get_patterns() {
    let generated = emit_checked_alias_source(
        r#"
type Entry = {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}
type EntryMap = Map<string, Entry>

function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}

function scoreAlias(entries: EntryMap, key: string) -> int {
    match entries.get(key) {
        case Some(entry) => entry.values.sorted()[0] * 1000 + entry.nested.values.sorted()[0] * 100 + entry.nested.callback(7, ...[1, 2])
        case None => 0
    }
}

function scoreDirect(
    entries: Map<string, {
        values: Array<int>,
        nested: { values: Array<int>, callback: (int, ...int) -> int }
    }>,
    key: string
) -> int {
    match entries.get(k: key) {
        case Some(entry) => entry.values.sorted()[0] * 1000 + entry.nested.values.sorted()[0] * 100 + entry.nested.callback(7, ...[1, 2])
        case None => 0
    }
}

function main() -> int {
    let base: EntryMap = map {
        "entry": {
            values: [3, 1, 2],
            nested: { values: [6, 4, 5], callback: sum }
        }
    }
    let aliasValue = scoreAlias(base, "entry")
    let directValue = scoreDirect(base, "entry")

    let mut entries: EntryMap = base
    if true {
        entries = map {
            "entry": {
                values: [9, 7, 8],
                nested: { values: [12, 10, 11], callback: sum }
            }
        }
    } else {
        entries = map {}
    }
    entries.insert(
        "entry",
        {
            values: [9, 7, 8],
            nested: { values: [12, 10, 11], callback: sum }
        }
    )
    let key = "entry"
    let mutableValue = match entries.get(key) {
        case Some(entry) => entry.values.sorted()[0] * 1000 + entry.nested.values.sorted()[0] * 100 + entry.nested.callback(20, ...[3, 4])
        case None => 0
    }
    aliasValue * 1000000 + directValue * 1000 + mutableValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        1411418027,
        "typed Map record descendant receiver and callable metadata",
    );

    let imported = emit_checked_alias_source_with_files(
        r#"
import selectedEntries { selected }
import namespaceEntries as namespace

function score(entryMap: Map<string, {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}>) -> int {
    match entryMap.get("entry") {
        case Some(entry) => entry.values.sorted()[0] * 1000 + entry.nested.values.sorted()[0] * 100 + entry.nested.callback(1, ...[2, 3])
        case None => 0
    }
}

function main() -> int {
    let selectedValue = match selected.get("entry") {
        case Some(entry) => entry.values.sorted()[0] * 1000 + entry.nested.values.sorted()[0] * 100 + entry.nested.callback(1, ...[2, 3])
        case None => 0
    }
    let namespaceValue = match namespace.entries.get("entry") {
        case Some(entry) => entry.values.sorted()[0] * 1000 + entry.nested.values.sorted()[0] * 100 + entry.nested.callback(1, ...[2, 3])
        case None => 0
    }
    selectedValue * 10000 + namespaceValue
}
main()
"#,
        &[
            (
                "selectedEntries.tpz",
                r#"
type Entry = {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}
type EntryMap = Map<string, Entry>
function sum(seed: int, ...xs: int) -> int { seed + xs[0] + xs[1] }
export let selected: EntryMap = map {
    "entry": {
        values: [4, 2, 3],
        nested: { values: [7, 5, 6], callback: sum }
    }
}
"#,
            ),
            (
                "namespaceEntries.tpz",
                r#"
type Entry = {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}
type EntryMap = Map<string, Entry>
function sum(seed: int, ...xs: int) -> int { seed + xs[0] + xs[1] }
export let entries: EntryMap = map {
    "entry": {
        values: [4, 2, 3],
        nested: { values: [7, 5, 6], callback: sum }
    }
}
"#,
            ),
        ],
    );
    assert_generated_python_ok_int(
        &imported,
        25062506,
        "selected and namespace imported typed Map record descendant metadata",
    );
}

#[test]
fn typed_maps_preserve_declared_option_record_value_metadata_through_get_patterns() {
    let generated = emit_checked_alias_source(
        r#"
type Payload = {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}
type PayloadMap = Map<string, Option<Payload>>

function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}

function scoreAlias(values: PayloadMap, key: string) -> int {
    match values.get(key) {
        case Some(payload) => {
            let direct = payload?.values?.sorted()?.length ?? 0
            let nested = payload?.nested?.values?.sorted()?.length ?? 0
            let called = payload?.nested?.callback(7, ...[1, 2]) ?? 0
            direct * 10000 + nested * 100 + called
        }
        case None => 0
    }
}

function scoreDirect(values: Map<string, Option<{
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}>>, key: string) -> int {
    match values.get(k: key) {
        case Some(payload) => {
            let direct = payload?.values?.sorted()?.length ?? 0
            let nested = payload?.nested?.values?.sorted()?.length ?? 0
            let called = payload?.nested?.callback(7, ...[1, 2]) ?? 0
            direct * 10000 + nested * 100 + called
        }
        case None => 0
    }
}

function main() -> int {
    let base: PayloadMap = map {
        "payload": Some({
            values: [3, 1, 2],
            nested: { values: [6, 4, 5], callback: sum }
        })
    }
    let aliasValue = scoreAlias(base, "payload")
    let directValue = scoreDirect(base, "payload")

    let mut values: PayloadMap = base
    if true {
        values = map {
            "payload": Some({
                values: [9, 7, 8],
                nested: { values: [12, 10, 11], callback: sum }
            })
        }
    } else {
        values = map {}
    }
    values.insert(
        "payload",
        Some({
            values: [9, 7, 8],
            nested: { values: [12, 10, 11], callback: sum }
        })
    )
    let key = "payload"
    let mutableValue = match values.get(key) {
        case Some(payload) => {
            let direct = payload?.values?.sorted()?.length ?? 0
            let nested = payload?.nested?.values?.sorted()?.length ?? 0
            let called = payload?.nested?.callback(20, ...[3, 4]) ?? 0
            direct * 10000 + nested * 100 + called
        }
        case None => 0
    }
    aliasValue * 1000000 + directValue * 1000 + mutableValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        30340340327,
        "typed Map Option-record descendant receiver and callable metadata",
    );

    let imported = emit_checked_alias_source_with_files(
        r#"
import selectedPayloads { selected }
import namespacePayloads as namespace

function main() -> int {
    let selectedValue = match selected.get("payload") {
        case Some(payload) => {
            let direct = payload?.values?.sorted()?.length ?? 0
            let nested = payload?.nested?.values?.sorted()?.length ?? 0
            let called = payload?.nested?.callback(1, ...[2, 3]) ?? 0
            direct * 10000 + nested * 100 + called
        }
        case None => 0
    }
    let namespaceValue = match namespace.values.get("payload") {
        case Some(payload) => {
            let direct = payload?.values?.sorted()?.length ?? 0
            let nested = payload?.nested?.values?.sorted()?.length ?? 0
            let called = payload?.nested?.callback(1, ...[2, 3]) ?? 0
            direct * 10000 + nested * 100 + called
        }
        case None => 0
    }
    selectedValue * 100000 + namespaceValue
}
main()
"#,
        &[
            (
                "selectedPayloads.tpz",
                r#"
type Payload = {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}
type PayloadMap = Map<string, Option<Payload>>
function sum(seed: int, ...xs: int) -> int { seed + xs[0] + xs[1] }
export let selected: PayloadMap = map {
    "payload": Some({
        values: [4, 2, 3],
        nested: { values: [7, 5, 6], callback: sum }
    })
}
"#,
            ),
            (
                "namespacePayloads.tpz",
                r#"
type Payload = {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}
type PayloadMap = Map<string, Option<Payload>>
function sum(seed: int, ...xs: int) -> int { seed + xs[0] + xs[1] }
export let values: PayloadMap = map {
    "payload": Some({
        values: [4, 2, 3],
        nested: { values: [7, 5, 6], callback: sum }
    })
}
"#,
            ),
        ],
    );
    assert_generated_python_ok_int(
        &imported,
        3030630306,
        "selected and namespace imported typed Map Option-record descendant metadata",
    );
}

#[test]
fn typed_maps_preserve_declared_result_record_value_metadata_through_nested_patterns() {
    let generated = emit_checked_alias_source(
        r#"
type Payload = {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}
type PayloadMap = Map<string, Result<Payload, string>>

function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}

function scoreAlias(values: PayloadMap, key: string) -> int {
    match values.get(key) {
        case Some(outcome) => match outcome {
            case Ok(payload) => {
                let direct = payload.values.sorted().length
                let nested = payload.nested.values.sorted().length
                let called = payload.nested.callback(7, ...[1, 2])
                direct * 10000 + nested * 100 + called
            }
            case Err(_) => 0
        }
        case None => 0
    }
}

function scoreDirect(values: Map<string, Result<{
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}, string>>, key: string) -> int {
    match values.get(k: key) {
        case Some(outcome) => match outcome {
            case Ok(payload) => {
                let direct = payload.values.sorted().length
                let nested = payload.nested.values.sorted().length
                let called = payload.nested.callback(7, ...[1, 2])
                direct * 10000 + nested * 100 + called
            }
            case Err(_) => 0
        }
        case None => 0
    }
}

function main() -> int {
    let base: PayloadMap = map {
        "payload": Ok({
            values: [3, 1, 2],
            nested: { values: [6, 4, 5], callback: sum }
        })
    }
    let aliasValue = scoreAlias(base, "payload")
    let directValue = scoreDirect(base, "payload")

    let mut values: PayloadMap = base
    if true {
        values = map {
            "payload": Ok({
                values: [9, 7, 8],
                nested: { values: [12, 10, 11], callback: sum }
            })
        }
    } else {
        values = map {}
    }
    values.insert(
        "payload",
        Ok({
            values: [9, 7, 8],
            nested: { values: [12, 10, 11], callback: sum }
        })
    )
    let key = "payload"
    let mutableValue = match values.get(key) {
        case Some(outcome) => match outcome {
            case Ok(payload) => {
                let direct = payload.values.sorted().length
                let nested = payload.nested.values.sorted().length
                let called = payload.nested.callback(20, ...[3, 4])
                direct * 10000 + nested * 100 + called
            }
            case Err(_) => 0
        }
        case None => 0
    }
    aliasValue * 1000000 + directValue * 1000 + mutableValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        30340340327,
        "typed Map Result-record nested pattern receiver and callable metadata",
    );

    let imported = emit_checked_alias_source_with_files(
        r#"
import selectedPayloads { selected }
import namespacePayloads as namespace

function score(values: Map<string, Result<{
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}, string>>) -> int {
    match values.get("payload") {
        case Some(outcome) => match outcome {
            case Ok(payload) => {
                let direct = payload.values.sorted().length
                let nested = payload.nested.values.sorted().length
                let called = payload.nested.callback(1, ...[2, 3])
                direct * 10000 + nested * 100 + called
            }
            case Err(_) => 0
        }
        case None => 0
    }
}

function main() -> int {
    let selectedValue = score(selected)
    let namespaceValue = score(namespace.values)
    selectedValue * 100000 + namespaceValue
}
main()
"#,
        &[
            (
                "selectedPayloads.tpz",
                r#"
type Payload = {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}
type PayloadMap = Map<string, Result<Payload, string>>
function sum(seed: int, ...xs: int) -> int { seed + xs[0] + xs[1] }
export let selected: PayloadMap = map {
    "payload": Ok({
        values: [4, 2, 3],
        nested: { values: [7, 5, 6], callback: sum }
    })
}
"#,
            ),
            (
                "namespacePayloads.tpz",
                r#"
type Payload = {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}
type PayloadMap = Map<string, Result<Payload, string>>
function sum(seed: int, ...xs: int) -> int { seed + xs[0] + xs[1] }
export let values: PayloadMap = map {
    "payload": Ok({
        values: [4, 2, 3],
        nested: { values: [7, 5, 6], callback: sum }
    })
}
"#,
            ),
        ],
    );
    assert_generated_python_ok_int(
        &imported,
        3030630306,
        "selected and namespace imported typed Map Result-record nested pattern metadata",
    );
}

#[test]
fn result_record_function_returns_preserve_metadata_through_ok_patterns() {
    let generated = emit_checked_alias_source(
        r#"
type Payload = {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}
type PayloadResult = Result<Payload, string>

function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}

function makeDeclared(seed: int) -> PayloadResult {
    Ok({
        values: [3, 1, 2],
        nested: { values: [6, 4, 5], callback: sum }
    })
}

function makeDirect(seed: int) -> Result<{
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}, string> {
    Ok({
        values: [3, 1, 2],
        nested: { values: [6, 4, 5], callback: sum }
    })
}

function makeObserved(seed: int) {
    Ok({
        values: [3, 1, 2],
        nested: { values: [6, 4, 5], callback: sum }
    })
}

function main() -> int {
    let declaredValue = match makeDeclared(7) {
        case Ok(payload) => {
            let direct = payload.values.sorted().length
            let nested = payload.nested.values.sorted().length
            let called = payload.nested.callback(7, ...[1, 2])
            direct * 10000 + nested * 100 + called
        }
        case Err(_) => 0
    }
    let directValue = match makeDirect(8) {
        case Ok(payload) => {
            let direct = payload.values.sorted().length
            let nested = payload.nested.values.sorted().length
            let called = payload.nested.callback(8, ...[1, 2])
            direct * 10000 + nested * 100 + called
        }
        case Err(_) => 0
    }
    let observed = makeObserved(9)
    let observedValue = match observed {
        case Ok(payload) => {
            let direct = payload.values.sorted().length
            let nested = payload.nested.values.sorted().length
            let called = payload.nested.callback(9, ...[1, 2])
            direct * 10000 + nested * 100 + called
        }
        case Err(_) => 0
    }
    declaredValue * 1000000000 + directValue * 1000000 + observedValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        30340311030312,
        "declared and observed Result-record function return metadata",
    );

    let imported = emit_checked_alias_source_with_files(
        r#"
import selectedPayloads { makeSelected }
import namespacePayloads as namespace

function main() -> int {
    let selectedValue = match makeSelected(4) {
        case Ok(payload) => {
            let direct = payload.values.sorted().length
            let nested = payload.nested.values.sorted().length
            let called = payload.nested.callback(4, ...[2, 3])
            direct * 10000 + nested * 100 + called
        }
        case Err(_) => 0
    }
    let namespaceValue = match namespace.make(5) {
        case Ok(payload) => {
            let direct = payload.values.sorted().length
            let nested = payload.nested.values.sorted().length
            let called = payload.nested.callback(5, ...[2, 3])
            direct * 10000 + nested * 100 + called
        }
        case Err(_) => 0
    }
    selectedValue * 100000 + namespaceValue
}
main()
"#,
        &[
            (
                "selectedPayloads.tpz",
                r#"
export type Payload = {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}
export type PayloadResult = Result<Payload, string>
function sum(seed: int, ...xs: int) -> int { seed + xs[0] + xs[1] }
export function makeSelected(seed: int) -> PayloadResult {
    Ok({
        values: [4, 2, 3],
        nested: { values: [7, 5, 6], callback: sum }
    })
}
"#,
            ),
            (
                "namespacePayloads.tpz",
                r#"
function sum(seed: int, ...xs: int) -> int { seed + xs[0] + xs[1] }
export function make(seed: int) {
    Ok({
        values: [4, 2, 3],
        nested: { values: [7, 5, 6], callback: sum }
    })
}
"#,
            ),
        ],
    );
    assert_generated_python_ok_int(
        &imported,
        3030930310,
        "selected declared and namespace observed Result-record function metadata",
    );
}

#[test]
fn composed_wrapper_paths_preserve_record_metadata_without_specialized_fields() {
    let generated = emit_checked_alias_source(
        r#"
type Payload = {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}
type ResultOption = Result<Option<Payload>, string>
type OptionResult = Option<Result<Payload, string>>

function sum(seed: int, ...xs: int) -> int {
    let mut total = seed
    for x in xs {
        total = total + x
    }
    total
}

function makeResultOption() -> ResultOption {
    Ok(Some({
        values: [3, 1, 2],
        nested: { values: [6, 4, 5], callback: sum }
    }))
}

function makeOptionResult() {
    Some(Ok({
        values: [3, 1, 2],
        nested: { values: [6, 4, 5], callback: sum }
    }))
}

function score(payload: Payload, seed: int) -> int {
    let direct = payload.values.sorted().length
    let nested = payload.nested.values.sorted().length
    let called = payload.nested.callback(seed, ...[1, 2])
    direct * 10000 + nested * 100 + called
}

function main() -> int {
    let resultOption = match makeResultOption() {
        case Ok(maybe) => match maybe {
            case Some(payload) => score(payload, 7)
            case None => 0
        }
        case Err(_) => 0
    }

    let observed = makeOptionResult()
    let optionResult = match observed {
        case Some(outcome) => match outcome {
            case Ok(payload) => score(payload, 8)
            case Err(_) => 0
        }
        case None => 0
    }

    let resultOptionMap: Map<string, Result<Option<Payload>, string>> = map {
        "payload": makeResultOption()
    }
    let fromResultOptionMap = match resultOptionMap.get("payload") {
        case Some(outcome) => match outcome {
            case Ok(maybe) => match maybe {
                case Some(payload) => score(payload, 9)
                case None => 0
            }
            case Err(_) => 0
        }
        case None => 0
    }

    let optionResultMap: Map<string, Option<Result<Payload, string>>> = map {
        "payload": makeOptionResult()
    }
    let fromOptionResultMap = match optionResultMap.get("payload") {
        case Some(maybe) => match maybe {
            case Some(outcome) => match outcome {
                case Ok(payload) => score(payload, 10)
                case Err(_) => 0
            }
            case None => 0
        }
        case None => 0
    }

    resultOption * 1000000 + optionResult * 1000 + fromResultOptionMap + fromOptionResultMap
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        30340371625,
        "composed Result-Option and Option-Result record wrapper metadata",
    );

    let imported = emit_checked_alias_source_with_files(
        r#"
import selectedPayloads { makeSelected }
import namespacePayloads as namespace

function main() -> int {
    let selectedValue = match makeSelected() {
        case Ok(maybe) => match maybe {
            case Some(payload) => {
                let direct = payload.values.sorted().length
                let nested = payload.nested.values.sorted().length
                let called = payload.nested.callback(4, ...[2, 3])
                direct * 10000 + nested * 100 + called
            }
            case None => 0
        }
        case Err(_) => 0
    }
    let namespaceValue = match namespace.make() {
        case Some(outcome) => match outcome {
            case Ok(payload) => {
                let direct = payload.values.sorted().length
                let nested = payload.nested.values.sorted().length
                let called = payload.nested.callback(5, ...[2, 3])
                direct * 10000 + nested * 100 + called
            }
            case Err(_) => 0
        }
        case None => 0
    }
    selectedValue * 100000 + namespaceValue
}
main()
"#,
        &[
            (
                "selectedPayloads.tpz",
                r#"
export type Payload = {
    values: Array<int>,
    nested: { values: Array<int>, callback: (int, ...int) -> int }
}
export type SelectedResult = Result<Option<Payload>, string>
function sum(seed: int, ...xs: int) -> int { seed + xs[0] + xs[1] }
export function makeSelected() -> SelectedResult {
    Ok(Some({
        values: [4, 2, 3],
        nested: { values: [7, 5, 6], callback: sum }
    }))
}
"#,
            ),
            (
                "namespacePayloads.tpz",
                r#"
function sum(seed: int, ...xs: int) -> int { seed + xs[0] + xs[1] }
export function make() {
    Some(Ok({
        values: [4, 2, 3],
        nested: { values: [7, 5, 6], callback: sum }
    }))
}
"#,
            ),
        ],
    );
    assert_generated_python_ok_int(
        &imported,
        3030930310,
        "selected and namespace composed wrapper record metadata",
    );
}
