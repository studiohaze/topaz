use super::*;

#[test]
fn imported_function_parameter_mutations_invalidate_argument_metadata() {
    let util = r#"
export function reverseItems(items: Array<(int) -> int>) -> () {
    let mut local = items
    local.reverse()
}
"#;
    let selected = emit_checked_alias_source_with_files(
        r#"
import util { reverseItems }
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function main() -> int {
    let mut callbacks = [first, second]
    let product = {
        done: reverseItems(callbacks),
        value: [0].map(callbacks[0])[0]
    }
    product.value
}
main()
"#,
        &[("util.tpz", util)],
    );
    assert_generated_python_ok_int(
        &selected,
        2,
        "selected imported function parameter mutation metadata invalidation",
    );

    let namespace = emit_checked_alias_source_with_files(
        r#"
import util as helpers
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function main() -> int {
    let mut callbacks = [first, second]
    let product = {
        done: helpers.reverseItems(callbacks),
        value: [0].map(callbacks[0])[0]
    }
    product.value
}
main()
"#,
        &[("util.tpz", util)],
    );
    assert_generated_python_ok_int(
        &namespace,
        2,
        "namespace imported function parameter mutation metadata invalidation",
    );
}

#[test]
fn exported_callable_value_parameter_mutations_invalidate_argument_metadata() {
    let util = r#"
function reverseItems(items: Array<(int) -> int>) -> () {
    let mut local = items
    local.reverse()
}
export let reverseAlias = reverseItems
"#;
    let selected = emit_checked_alias_source_with_files(
        r#"
import util { reverseAlias }
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function main() -> int {
    let mut callbacks = [first, second]
    let product = {
        done: reverseAlias(callbacks),
        value: [0].map(callbacks[0])[0]
    }
    product.value
}
main()
"#,
        &[("util.tpz", util)],
    );
    assert_generated_python_ok_int(
        &selected,
        2,
        "selected imported callable value parameter mutation metadata invalidation",
    );

    let namespace = emit_checked_alias_source_with_files(
        r#"
import util as helpers
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function main() -> int {
    let mut callbacks = [first, second]
    let product = {
        done: helpers.reverseAlias(callbacks),
        value: [0].map(callbacks[0])[0]
    }
    product.value
}
main()
"#,
        &[("util.tpz", util)],
    );
    assert_generated_python_ok_int(
        &namespace,
        2,
        "namespace imported callable value parameter mutation metadata invalidation",
    );
}

#[test]
fn transitive_function_parameter_mutations_are_declaration_order_independent() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function outer(items: Array<(int) -> int>) -> () { middle(items) }
function middle(items: Array<(int) -> int>) -> () { inner(items) }
function inner(items: Array<(int) -> int>) -> () {
    let mut local = items
    local.reverse()
}
function main() -> int {
    let mut callbacks = [first, second]
    let product = {
        done: outer(callbacks),
        value: [0].map(callbacks[0])[0]
    }
    product.value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        2,
        "transitive function parameter mutation declaration-order-independent metadata",
    );
}

#[test]
fn imported_transitive_function_parameter_mutations_invalidate_argument_metadata() {
    let selected = emit_checked_alias_source_with_files(
        r#"
import util { outer }
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function localWrapper(items: Array<(int) -> int>) -> () { outer(items) }
function main() -> int {
    let mut callbacks = [first, second]
    let product = {
        done: localWrapper(callbacks),
        value: [0].map(callbacks[0])[0]
    }
    product.value
}
main()
"#,
        &[(
            "util.tpz",
            r#"
export function outer(items: Array<(int) -> int>) -> () { inner(items) }
function inner(items: Array<(int) -> int>) -> () {
    let mut local = items
    local.reverse()
}
"#,
        )],
    );
    assert_generated_python_ok_int(
        &selected,
        2,
        "imported transitive function parameter mutation metadata invalidation",
    );
}

#[test]
fn nested_function_parameter_mutations_survive_wrappers_and_aliases() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function main() -> int {
    function outer(items: Array<(int) -> int>) -> () { middle(items) }
    function middle(items: Array<(int) -> int>) -> () {
        let mut local = items
        local.reverse()
    }

    let mut fromNested = [first, second]
    let nested = {
        done: outer(fromNested),
        value: [0].map(fromNested[0])[0]
    }

    let alias = outer
    let mut fromAlias = [first, second]
    let aliased = {
        done: alias(fromAlias),
        value: [0].map(fromAlias[0])[0]
    }

    nested.value * 10 + aliased.value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        22,
        "nested function wrapper and alias parameter mutation metadata invalidation",
    );
}

#[test]
fn conditional_callable_values_join_parameter_mutation_effects() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function reverseItems(items: Array<(int) -> int>) -> () {
    let mut local = items
    local.reverse()
}
function keepItems(items: Array<(int) -> int>) -> () { () }
function main() -> int {
    let ifMutator = if true { reverseItems } else { keepItems }
    let mut fromIf = [first, second]
    let ifProduct = {
        done: ifMutator(fromIf),
        value: [0].map(fromIf[0])[0]
    }

    let choice: int = 1
    let matchMutator = match choice {
        case 0 => keepItems
        case _ => reverseItems
    }
    let mut fromMatch = [first, second]
    let matchProduct = {
        done: matchMutator(fromMatch),
        value: [0].map(fromMatch[0])[0]
    }

    ifProduct.value * 10 + matchProduct.value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        22,
        "conditional callable parameter mutation effect join",
    );
}

#[test]
fn stored_typed_lambda_parameter_mutations_invalidate_argument_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function reverseItems(items: Array<(int) -> int>) -> () {
    let mut local = items
    local.reverse()
}
function main() -> int {
    let direct = (items: Array<(int) -> int>) => {
        let mut local = items
        local.reverse()
    }
    let mut fromDirect = [first, second]
    let directProduct = {
        done: direct(fromDirect),
        value: [0].map(fromDirect[0])[0]
    }

    let wrapper = (items: Array<(int) -> int>) => reverseItems(items)
    let mut fromWrapper = [first, second]
    let wrapperProduct = {
        done: wrapper(fromWrapper),
        value: [0].map(fromWrapper[0])[0]
    }

    directProduct.value * 10 + wrapperProduct.value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        22,
        "stored typed lambda direct and transitive parameter mutation metadata invalidation",
    );
}

#[test]
fn contextually_typed_lambda_parameter_mutations_invalidate_argument_metadata() {
    let generated = emit_checked_alias_source(
        r#"
function first(x: int) -> int { 1 }
function second(x: int) -> int { 2 }
function reverseItems(items: Array<(int) -> int>) -> () {
    let mut local = items
    local.reverse()
}
type Mutator = (Array<(int) -> int>) -> ()
function main() -> int {
    let direct: (Array<(int) -> int>) -> () = items => {
        let mut local = items
        local.reverse()
    }
    let mut fromDirect = [first, second]
    let directProduct = {
        done: direct(fromDirect),
        value: [0].map(fromDirect[0])[0]
    }

    let wrapper: (Array<(int) -> int>) -> () = items => reverseItems(items)
    let mut fromWrapper = [first, second]
    let wrapperProduct = {
        done: wrapper(fromWrapper),
        value: [0].map(fromWrapper[0])[0]
    }

    let aliasTyped: Mutator = items => {
        let mut local = items
        local.reverse()
    }
    let mut fromAlias = [first, second]
    let aliasProduct = {
        done: aliasTyped(fromAlias),
        value: [0].map(fromAlias[0])[0]
    }

    let statement: (Array<(int) -> int>) -> () = items => {
        let mut local = items
        local.reverse()
    }
    let mut fromStatement = [first, second]
    let statementProduct = {
        done: statement(fromStatement),
        value: [0].map(fromStatement[0])[0]
    }

    directProduct.value * 1000 + wrapperProduct.value * 100 + aliasProduct.value * 10 + statementProduct.value
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        2222,
        "contextually typed lambda parameter mutation metadata invalidation",
    );
}
