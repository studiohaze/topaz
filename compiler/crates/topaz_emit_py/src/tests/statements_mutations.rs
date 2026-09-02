use super::*;

#[test]
fn concurrent_join_invalidates_direct_arm_array_mutation_metadata() {
    let generated = emit_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function main() -> int {
    let mut fromJoin = [first, second]
    let joined = {
        done: concurrent {
            mutate: fromJoin.reverse()
        },
        value: [0].map(fromJoin[0])[0]
    }

    let mut fromTimeout = [first, second]
    let timed = {
        done: concurrent(timeout: 1m) {
            mutate: fromTimeout.reverse()
        } else {
            { mutate: () }
        },
        value: [0].map(fromTimeout[0])[0]
    }

    joined.value * 10 + timed.value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        22,
        "concurrent join direct arm array mutation metadata invalidation",
    );
}

#[test]
fn concurrent_else_invalidates_direct_array_mutation_metadata() {
    let generated = emit_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function main() -> int {
    let mut fromCertainElse = [first, second]
    let certainElse = {
        done: concurrent(timeout: 0ms) {
            left: 1
            right: 2
        } else {
            { left: fromCertainElse.reverse(), right: () }
        },
        value: [0].map(fromCertainElse[0])[0]
    }

    let mut fromSkippedElse = [first, second]
    let skippedElse = {
        done: concurrent(timeout: 0ms) {
            value: 1
        } else {
            { value: fromSkippedElse.reverse() }
        },
        value: [0].map(fromSkippedElse[0])[0]
    }

    certainElse.value * 10 + skippedElse.value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        21,
        "concurrent else direct array mutation metadata invalidation",
    );
}

#[test]
fn ordered_subexpressions_invalidate_direct_array_mutation_metadata() {
    let generated = emit_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function take(done: unit, value: int) -> int { value }
function main() -> int {
    let mut fromArgs = [first, second]
    let argsResult = take(fromArgs.reverse(), [0].map(fromArgs[0])[0])

    let mut fromFields = [first, second]
    let fieldsResult = {
        done: fromFields.reverse(),
        value: [0].map(fromFields[0])[0]
    }

    argsResult * 10 + fieldsResult.value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        22,
        "ordered subexpression direct array mutation metadata invalidation",
    );
}

#[test]
fn immediate_lambda_mutation_effects_preserve_delayed_lambda_boundary() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function main() -> int {
    let mut fromImmediate = [first, second]
    let immediate = {
        done: (() => fromImmediate.reverse())(),
        value: [0].map(fromImmediate[0])[0]
    }

    let mut fromDelayed = [first, second]
    let returned = (() => (() => fromDelayed.reverse()))()
    let delayedValue = [0].map(fromDelayed[0])[0]

    immediate.value * 10 + delayedValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        21,
        "immediate lambda mutation and delayed returned lambda boundary",
    );
}

#[test]
fn immediate_lambda_parameter_mutations_invalidate_argument_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function main() -> int {
    let mut fromParameter = [first, second]
    let parameter = {
        done: ((items: Array<(int) -> int>) => {
            let mut local = items
            local.reverse()
        })(fromParameter),
        value: [0].map(fromParameter[0])[0]
    }

    let mut fromNestedParameter = [first, second]
    let nestedParameter = {
        done: ((outer: Array<(int) -> int>) =>
            ((inner: Array<(int) -> int>) => {
                let mut local = inner
                local.reverse()
            })(outer)
        )(fromNestedParameter),
        value: [0].map(fromNestedParameter[0])[0]
    }

    parameter.value * 10 + nestedParameter.value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        22,
        "immediate lambda parameter mutation argument metadata invalidation",
    );
}

#[test]
fn known_function_parameter_mutations_invalidate_argument_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function reverseItems(items: Array<(int) -> int>) -> () {
    let mut local = items
    local.reverse()
}
function reverseNamed(prefix: int, items: Array<(int) -> int>) -> () {
    let mut local = items
    local.reverse()
}
function inspectItems(items: Array<(int) -> int>) -> int { items.length }
function main() -> int {
    let mut fromPosition = [first, second]
    let positional = {
        done: reverseItems(fromPosition),
        value: [0].map(fromPosition[0])[0]
    }

    let mut fromName = [first, second]
    let named = {
        done: reverseNamed(prefix: 0, items: fromName),
        value: [0].map(fromName[0])[0]
    }

    let mut fromRead = [first, second]
    let read = {
        done: inspectItems(fromRead),
        value: [0].map(fromRead[0])[0]
    }

    positional.value * 100 + named.value * 10 + read.value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        221,
        "known function parameter mutation argument metadata invalidation",
    );
}

#[test]
fn mutable_local_array_alias_reassignment_targets_the_current_parameter() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function reassignAndReverse(
    firstItems: Array<(int) -> int>,
    secondItems: Array<(int) -> int>
) -> () {
    let mut selected = firstItems
    selected = secondItems
    selected.reverse()
}
function replaceFirst(items: Array<(int) -> int>, callback: (int) -> int) -> () {
    let mut local = items
    local[0] = callback
}
function main() -> int {
    let mut untouched = [first, second]
    let mut reassigned = [first, second]
    let reassignProduct = {
        done: reassignAndReverse(untouched, reassigned),
        firstValue: [0].map(untouched[0])[0],
        secondValue: [0].map(reassigned[0])[0]
    }

    let mut indexed = [first, first]
    let indexProduct = {
        done: replaceFirst(indexed, second),
        value: [0].map(indexed[0])[0]
    }

    reassignProduct.firstValue * 100 + reassignProduct.secondValue * 10 + indexProduct.value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        122,
        "mutable local Array alias reassignment and indexed mutation effects",
    );
}

#[test]
fn branch_joined_array_aliases_preserve_every_reachable_parameter() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function chooseIf(
    chooseSecond: bool,
    initial: Array<(int) -> int>,
    secondItems: Array<(int) -> int>,
    thirdItems: Array<(int) -> int>
) -> () {
    let mut selected = initial
    if chooseSecond {
        selected = secondItems
    } else {
        selected = thirdItems
    }
    selected.reverse()
}
function chooseMatch(
    choice: int,
    initial: Array<(int) -> int>,
    secondItems: Array<(int) -> int>,
    thirdItems: Array<(int) -> int>
) -> () {
    let mut selected = initial
    match choice {
        case 0 => {
            selected = secondItems
            ()
        }
        case _ => {
            selected = thirdItems
            ()
        }
    }
    selected.reverse()
}
function main() -> int {
    let mut ifInitial = [first, second]
    let mut ifSecond = [first, second]
    let mut ifThird = [first, second]
    let ifProduct = {
        done: chooseIf(true, ifInitial, ifSecond, ifThird),
        secondValue: [0].map(ifSecond[0])[0],
        thirdValue: [0].map(ifThird[0])[0]
    }

    let mut matchInitial = [first, second]
    let mut matchSecond = [first, second]
    let mut matchThird = [first, second]
    let matchProduct = {
        done: chooseMatch(0, matchInitial, matchSecond, matchThird),
        secondValue: [0].map(matchSecond[0])[0],
        thirdValue: [0].map(matchThird[0])[0]
    }

    ifProduct.secondValue * 1000 + ifProduct.thirdValue * 100 + matchProduct.secondValue * 10 + matchProduct.thirdValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        2121,
        "if and match branch joined mutable Array alias effects",
    );
}

#[test]
fn iteration_and_concurrent_alias_joins_preserve_runtime_parameters() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function afterEmptyFor(
    firstItems: Array<(int) -> int>,
    secondItems: Array<(int) -> int>
) -> () {
    let mut selected = firstItems
    let empty: Array<int> = []
    for ignored in empty {
        selected = secondItems
    }
    selected.reverse()
}
function afterFalseWhile(
    firstItems: Array<(int) -> int>,
    secondItems: Array<(int) -> int>
) -> () {
    let mut selected = firstItems
    while false {
        selected = secondItems
    }
    selected.reverse()
}
function afterConcurrent(
    firstItems: Array<(int) -> int>,
    secondItems: Array<(int) -> int>,
    thirdItems: Array<(int) -> int>
) -> () {
    let mut selected = firstItems
    let joined = concurrent(timeout: 1m) {
        choose: {
            selected = secondItems
            1
        }
    } else {
        { choose: {
            selected = thirdItems
            0
        } }
    }
    selected.reverse()
}
function main() -> int {
    let mut forFirst = [first, second]
    let mut forSecond = [first, second]
    let forProduct = {
        done: afterEmptyFor(forFirst, forSecond),
        firstValue: [0].map(forFirst[0])[0],
        secondValue: [0].map(forSecond[0])[0]
    }

    let mut whileFirst = [first, second]
    let mut whileSecond = [first, second]
    let whileProduct = {
        done: afterFalseWhile(whileFirst, whileSecond),
        firstValue: [0].map(whileFirst[0])[0],
        secondValue: [0].map(whileSecond[0])[0]
    }

    let mut concurrentFirst = [first, second]
    let mut concurrentSecond = [first, second]
    let mut concurrentThird = [first, second]
    let concurrentProduct = {
        done: afterConcurrent(concurrentFirst, concurrentSecond, concurrentThird),
        secondValue: [0].map(concurrentSecond[0])[0],
        thirdValue: [0].map(concurrentThird[0])[0]
    }

    let forValue = forProduct.firstValue * 10 + forProduct.secondValue
    let whileValue = whileProduct.firstValue * 10 + whileProduct.secondValue
    let concurrentValue = concurrentProduct.secondValue * 10 + concurrentProduct.thirdValue
    forValue * 10000 + whileValue * 100 + concurrentValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        212_121,
        "zero-iteration and concurrent mutable Array alias joins",
    );
}

#[test]
fn lazy_and_optional_call_alias_joins_preserve_runtime_parameters() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function add(sum: int, value: int) -> int { sum + value }
function afterSkippedAnd(
    firstItems: Array<(int) -> int>,
    secondItems: Array<(int) -> int>
) -> () {
    let mut selected = firstItems
    let skipped = false && {
        selected = secondItems
        true
    }
    selected.reverse()
}
function afterSkippedOr(
    firstItems: Array<(int) -> int>,
    secondItems: Array<(int) -> int>
) -> () {
    let mut selected = firstItems
    let skipped = true || {
        selected = secondItems
        false
    }
    selected.reverse()
}
function afterSkippedCoalesce(
    firstItems: Array<(int) -> int>,
    secondItems: Array<(int) -> int>
) -> () {
    let mut selected = firstItems
    let skipped = Some(1) ?? {
        selected = secondItems
        0
    }
    selected.reverse()
}
function afterSkippedOptionalCall(
    firstItems: Array<(int) -> int>,
    secondItems: Array<(int) -> int>
) -> () {
    let mut selected = firstItems
    let none: Option<Array<int>> = None
    let skipped = none?.get(i: {
        selected = secondItems
        0
    })
    selected.reverse()
}
function afterSkippedOptionalPipe(
    firstItems: Array<(int) -> int>,
    secondItems: Array<(int) -> int>
) -> () {
    let mut selected = firstItems
    let none: Option<Array<int>> = None
    let skipped = 0 |> none?.reduce({
        selected = secondItems
        add
    })
    selected.reverse()
}
function main() -> int {
    let mut andFirst = [first, second]
    let mut andSecond = [first, second]
    let andProduct = {
        done: afterSkippedAnd(andFirst, andSecond),
        firstValue: [0].map(andFirst[0])[0],
        secondValue: [0].map(andSecond[0])[0]
    }

    let mut orFirst = [first, second]
    let mut orSecond = [first, second]
    let orProduct = {
        done: afterSkippedOr(orFirst, orSecond),
        firstValue: [0].map(orFirst[0])[0],
        secondValue: [0].map(orSecond[0])[0]
    }

    let mut coalesceFirst = [first, second]
    let mut coalesceSecond = [first, second]
    let coalesceProduct = {
        done: afterSkippedCoalesce(coalesceFirst, coalesceSecond),
        firstValue: [0].map(coalesceFirst[0])[0],
        secondValue: [0].map(coalesceSecond[0])[0]
    }

    let mut optionalFirst = [first, second]
    let mut optionalSecond = [first, second]
    let optionalProduct = {
        done: afterSkippedOptionalCall(optionalFirst, optionalSecond),
        firstValue: [0].map(optionalFirst[0])[0],
        secondValue: [0].map(optionalSecond[0])[0]
    }

    let mut pipeFirst = [first, second]
    let mut pipeSecond = [first, second]
    let pipeProduct = {
        done: afterSkippedOptionalPipe(pipeFirst, pipeSecond),
        firstValue: [0].map(pipeFirst[0])[0],
        secondValue: [0].map(pipeSecond[0])[0]
    }

    let andValue = andProduct.firstValue * 10 + andProduct.secondValue
    let orValue = orProduct.firstValue * 10 + orProduct.secondValue
    let coalesceValue = coalesceProduct.firstValue * 10 + coalesceProduct.secondValue
    let optionalValue = optionalProduct.firstValue * 10 + optionalProduct.secondValue
    let pipeValue = pipeProduct.firstValue * 10 + pipeProduct.secondValue
    andValue * 100000000 + orValue * 1000000 + coalesceValue * 10000 + optionalValue * 100 + pipeValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        2_121_212_121,
        "lazy and optional-call mutable Array alias joins",
    );
}

#[test]
fn deferred_alias_flow_runs_at_scope_drain() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function mutateBeforeDeferredRebind(
    firstItems: Array<(int) -> int>,
    secondItems: Array<(int) -> int>
) -> () {
    let mut selected = firstItems
    defer {
        selected = secondItems
        ()
    }
    selected.reverse()
}
function deferredMutationAfterRebind(
    firstItems: Array<(int) -> int>,
    secondItems: Array<(int) -> int>
) -> () {
    let mut selected = firstItems
    defer {
        selected.reverse()
        ()
    }
    selected = secondItems
}
function main() -> int {
    let mut beforeFirst = [first, second]
    let mut beforeSecond = [first, second]
    let beforeProduct = {
        done: mutateBeforeDeferredRebind(beforeFirst, beforeSecond),
        firstValue: [0].map(beforeFirst[0])[0],
        secondValue: [0].map(beforeSecond[0])[0]
    }

    let mut afterFirst = [first, second]
    let mut afterSecond = [first, second]
    let afterProduct = {
        done: deferredMutationAfterRebind(afterFirst, afterSecond),
        firstValue: [0].map(afterFirst[0])[0],
        secondValue: [0].map(afterSecond[0])[0]
    }

    let beforeValue = beforeProduct.firstValue * 10 + beforeProduct.secondValue
    let afterValue = afterProduct.firstValue * 10 + afterProduct.secondValue
    beforeValue * 100 + afterValue
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        2112,
        "deferred mutable Array alias flow at scope drain",
    );
}

#[test]
fn projected_array_aliases_preserve_outer_parameter_roots() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
record Bucket { items: Array<(int) -> int> }
function reverseItems(items: Array<(int) -> int>) -> () {
    let mut local = items
    local.reverse()
}
function reverseProjectedAlias(groups: Array<Array<(int) -> int>>) -> () {
    let mut selected = groups[0]
    selected.reverse()
}
function reverseProjectedCall(groups: Array<Array<(int) -> int>>) -> () {
    reverseItems(groups[0])
}
function reverseProjectedImmediateLambda(groups: Array<Array<(int) -> int>>) -> () {
    ((items: Array<(int) -> int>) => {
        let mut local = items
        local.reverse()
    })(groups[0])
}
function reverseProjectedMemberCall(groups: Array<Bucket>) -> () {
    reverseItems(groups[0].items)
}
function main() -> int {
    let mut aliasGroups = [[first, second]]
    let aliasProduct = {
        done: reverseProjectedAlias(aliasGroups),
        value: [0].map(aliasGroups[0][0])[0]
    }

    let mut callGroups = [[first, second]]
    let callProduct = {
        done: reverseProjectedCall(callGroups),
        value: [0].map(callGroups[0][0])[0]
    }

    let mut lambdaGroups = [[first, second]]
    let lambdaProduct = {
        done: reverseProjectedImmediateLambda(lambdaGroups),
        value: [0].map(lambdaGroups[0][0])[0]
    }

    let mut memberGroups = [Bucket { items: [first, second] }]
    let memberCallProduct = {
        done: reverseProjectedMemberCall(memberGroups),
        value: [0].map(memberGroups[0].items[0])[0]
    }

    aliasProduct.value * 1000 + callProduct.value * 100 + lambdaProduct.value * 10 + memberCallProduct.value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        2222,
        "projected mutable Array alias outer parameter roots",
    );
}
