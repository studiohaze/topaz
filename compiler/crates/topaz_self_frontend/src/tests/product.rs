use super::*;

#[test]
fn linked_stage2_complete_product_has_mechanical_manifest() {
    let host = InlineLoweringFixtureHost("export function answer() -> int {\n  40 + 2\n}\n");
    let product = preview_linked_stage2_compilation_product(
        &host,
        generated_request(),
        CompilationProfile::None,
    )
    .expect("complete self product");
    assert_eq!(product.status(), "completed");
    assert!(!product.generated_rust().is_empty());
    assert!(!product.typed.nodes.is_empty());
    assert!(!product.typed.resolved.exports.is_empty());
    assert!(!product.lowered.operations.is_empty());
    let manifest = encode_self_compilation_product_manifest(&product).expect("product manifest");
    validate_self_compilation_product_manifest(&manifest).expect("valid product manifest");
    let text = std::str::from_utf8(&manifest).expect("manifest UTF-8");
    assert!(text.contains("\"producer\":\"topaz-stage2\""));
    assert!(text.contains("\"self.c2-profile\""));
    assert!(!text.contains("\"rust."));
}

#[test]
fn linked_stage2_complete_product_executes_without_a_rust_front_end() {
    let host = InlineLoweringFixtureHost(
        "export function main() -> Result<int, string> {\n  Ok(40 + 2)\n}\n",
    );
    let product = preview_linked_stage2_compilation_product(
        &host,
        generated_request(),
        CompilationProfile::None,
    )
    .expect("complete executable self product");
    let (value, explicit_main) =
        execute_self_compilation_product(&product, &[]).expect("self product execution");
    assert!(explicit_main);
    assert!(matches!(
        value,
        Value::Ok(value) if matches!(&*value, Value::Int(42))
    ));
}

#[test]
fn linked_stage2_exported_values_initialize_once() {
    let host = InlineLoweringFixtureHost(
        "let mut initializations = 0\n\
             function initialize() -> int {\n\
               initializations += 1\n\
               print(\"initialized:{initializations}\")\n\
               initializations\n\
             }\n\
             export const base = 40\n\
             export let value: int = initialize()\n\
             export function main() -> Array<int> { [base + value, initializations] }\n",
    );
    let product = preview_linked_stage2_compilation_product(
        &host,
        generated_request(),
        CompilationProfile::None,
    )
    .expect("self product with exported value");
    let runtime_host = Rc::new(topaz_interp::TestHost::new());
    let (value, explicit_main) = execute_self_compilation_product_with_host_and_input(
        &product,
        &[],
        "",
        runtime_host.clone(),
    )
    .expect("self product exported value execution");
    let values = match value {
        Value::Array(values) => values
            .borrow()
            .iter()
            .map(|value| match value {
                Value::Int(value) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>(),
        _ => None,
    };
    assert_eq!(
        (explicit_main, values, runtime_host.stdout()),
        (true, Some(vec![41, 1]), vec!["initialized:1".to_string()],),
    );
}

#[test]
fn current_source_target_runtime_binds_named_default_and_spread_user_arguments() {
    let host = InlineLoweringFixtureHost(
        "function score(prefix: int, suffix: int = 5, ...rest: int) -> int {\n\
               prefix * 100 + suffix * 10 + rest.length\n\
             }\n\
             export function main() -> int {\n\
               score(suffix: 2, prefix: 1) + score(3, ...[7, 8], 9)\n\
             }\n",
    );
    let generated = preview_stage1_generated(&host, generated_request())
        .expect("current-source user argument product");
    let operations = parse_stage1_lowered_operations(&generated.response_root)
        .expect("current-source lowered operations");
    let ir = generated_ir_payload(&generated.generated_rust)
        .expect("current-source generated IR payload");
    let (value, explicit_main) = topaz_stage1_runtime::execute_product_program(ir, &[])
        .expect("current-source user argument execution");
    let result = match value {
        Value::Int(value) => Some(value),
        _ => None,
    };
    assert_eq!(
        (
            generated.status.as_str(),
            operations
                .iter()
                .filter(|operation| operation.kind == "binding/variadic-parameter")
                .count(),
            operations
                .iter()
                .filter(|operation| {
                    operation.kind == "expression/call"
                        && operation
                            .call_arguments
                            .iter()
                            .any(|argument| argument.starts_with("named|"))
                })
                .count(),
            operations
                .iter()
                .filter(|operation| {
                    operation.kind == "expression/call"
                        && operation
                            .call_arguments
                            .iter()
                            .any(|argument| argument.starts_with("spread|"))
                })
                .count(),
            explicit_main,
            result,
        ),
        ("completed", 1, 1, 1, true, Some(473)),
    );
}

#[test]
fn current_source_target_runtime_consumes_call_evaluation_and_builtin_binding_plans() {
    let host = InlineLoweringFixtureHost(
        "let mut order = 0\n\
             function mark(label: int, value: int) -> int {\n\
               order = order * 10 + label\n\
               value\n\
             }\n\
             function identity(value: int) -> int { value }\n\
             function add(left: int, right: int) -> int { left + right }\n\
             export function main() -> Array<int> {\n\
               let direct = identity(mark(2, 2))\n\
               let values = [40]\n\
               let member = (if mark(3, 0) == 0 { values } else { values }).get(mark(4, 0))\n\
             let absent: Option<Array<int>> = None\n\
             let skipped = absent?.get(mark(1, 0))\n\
             let present: Option<Array<int>> = Some([50])\n\
             let wrapped = present?.get(mark(5, 0))\n\
             let skippedPipe = mark(1, 0) |> absent?.get(mark(2, _))\n\
             let wrappedPipe = mark(3, 0) |> present?.get(mark(4, _))\n\
             let inserted = mark(6, 6) |> identity()\n\
               let replaced = mark(7, 7) |> identity(_)\n\
               let nestedPlaceholder = 8 |> identity({ _ })\n\
               let isolatedNestedPipe = 10 |> add(100 |> add(_, 1))\n\
               let reorderedBuffer = ByteBuffer.allocate(value: mark(9, 9), length: mark(8, 1))\n\
               let parsed = toInt(text: \" 42 \")\n\
               let buffer = ByteBuffer.allocate(length: 1)\n\
               let memberValue = match member { case Some(value) => value; case None => -1 }\n\
             let optionalCount = if skipped == None && wrapped != None && skippedPipe == None && wrappedPipe != None { 4 } else { -1 }\n\
               let parsedValue = match parsed { case Some(value) => value; case None => -1 }\n\
               [order, direct, memberValue, optionalCount, inserted, replaced, nestedPlaceholder, isolatedNestedPipe, reorderedBuffer.get(0), parsedValue, buffer.get(0)]\n\
             }\n",
    );
    let generated = preview_stage1_generated(&host, generated_request())
        .expect("current-source call evaluation product");
    let operations = parse_stage1_lowered_operations(&generated.response_root)
        .expect("current-source lowered operations");
    let ir = generated_ir_payload(&generated.generated_rust)
        .expect("current-source generated IR payload");
    let (value, explicit_main) = topaz_stage1_runtime::execute_product_program(ir, &[])
        .expect("current-source call evaluation execution");
    let result = match value {
        Value::Array(values) => values
            .borrow()
            .iter()
            .map(|value| match value {
                Value::Int(value) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>(),
        _ => None,
    };
    assert_eq!(
        (
            generated.status.as_str(),
            operations
                .iter()
                .filter(|operation| operation.call_optional)
                .count(),
            operations
                .iter()
                .filter(|operation| operation.kind == "expression/pipeline")
                .count(),
            operations
                .iter()
                .flat_map(|operation| &operation.call_arguments)
                .filter(|argument| argument.starts_with("inserted-lead|"))
                .count(),
            operations
                .iter()
                .flat_map(|operation| &operation.call_evaluations)
                .filter(|evaluation| evaluation.as_str() == "pipe-lead|-1")
                .count(),
            operations
                .iter()
                .flat_map(|operation| &operation.call_evaluations)
                .filter(|evaluation| evaluation.as_str() == "optional-guard|-1")
                .count(),
            explicit_main,
            result,
        ),
        (
            "completed",
            4,
            7,
            2,
            5,
            4,
            true,
            Some(vec![23451346798, 2, 40, 4, 6, 7, 8, 111, 9, 42, 0]),
        ),
    );
}

#[test]
fn current_source_target_runtime_executes_callable_members_and_callbacks() {
    let host = InlineLoweringFixtureHost(
        "function plusOne(value: int) -> int { value + 1 }\n\
             function fortyTwo() -> int { 42 }\n\
             function add(left: int, right: int) -> int { left + right }\n\
             function keepTwo(value: int) -> bool { value == 2 }\n\
             function optionStep(value: int) -> Option<int> { Some(value + 1) }\n\
             function resultStep(value: int) -> Result<int, string> { Ok(value + 2) }\n\
             function missing() -> string { \"missing\" }\n\
             function identityInt(value: int) -> int { value }\n\
             function timesTen(value: int) -> int { value * 10 }\n\
             function keepMap(key: string, value: int) -> bool { key == \"b\" && value == 2 }\n\
             export function main() -> Array<int> {\n\
               let callable = { map: plusOne, okOrElse: fortyTwo, apply: add, parse: toInt, emit: print }\n\
               let shadowMap = callable.map(40)\n\
               let shadowLazy = callable.okOrElse()\n\
               let callableField = callable.apply(20, 22)\n\
               let parsed = callable.parse(\" 43 \")\n\
               let firstClassGet = [44].get\n\
               let fetched = firstClassGet(0)\n\
               let mapped = [1, 2, 3].map(plusOne)\n\
               let offset = 10\n\
               let lambdaMapped = [1, 2].map((value) => value + offset)\n\
               let parsedMany = [\" 45 \"].map(toInt)\n\
               let composedValue = (plusOne >> plusOne)(40)\n\
               let composedMapped = [40].map(plusOne >> plusOne)\n\
               let selected = if false { plusOne } else { timesTen }\n\
               let selectedValue = selected(4)\n\
               let mut callbackCount = 0\n\
               let counted = (value: int) => { callbackCount += 1; value + callbackCount }\n\
               let countedValues = [10, 10].map(counted)\n\
               let filtered = [1, 2, 3].filter(keepTwo)\n\
               let reduced = [1, 2, 3].reduce(0, add)\n\
               let optionMapped = Some(40).map(plusOne)\n\
               let optionFlat = Some(40).flatMap(optionStep)\n\
               let resultMapped: Result<int, string> = Ok(40).map(plusOne)\n\
               let resultFlat: Result<int, string> = Ok(40).flatMap(resultStep)\n\
               let absent: Option<int> = None\n\
               let fallback = absent.okOrElse(missing)\n\
               let mut sorted = [3, 1, 2]\n\
               sorted.sortBy(identityInt)\n\
               let sortedCopy = [3, 1, 2].sortedBy(identityInt)\n\
               let mut retained = [1, 2, 3]\n\
               retained.retain(keepTwo)\n\
               let mut keyed = map { \"a\": 1, \"b\": 2 }\n\
               let mappedValues = keyed.mapValues(timesTen)\n\
               let filteredMap = keyed.filter(keepMap)\n\
               keyed.update(\"a\", 0, plusOne)\n\
               let mut mappedSource: Map<string, int> = map { \"a\": 1, \"b\": 2 }\n\
               let mut mappedCalls = 0\n\
               let mutateMapped = (value: int) => { mappedCalls += 1; mappedSource.insert(\"c\", 3); value * 10 }\n\
               let mappedSnapshot = mappedSource.mapValues(mutateMapped)\n\
               let mut filteredSource: Map<string, int> = map { \"a\": 1, \"b\": 2 }\n\
               let mutateFiltered = (key: string, value: int) => { filteredSource.insert(\"c\", value); key == \"b\" }\n\
               let filteredSnapshot = filteredSource.filter(mutateFiltered)\n\
               let mappedValue = match mapped.get(2) { case Some(value) => value; case None => -1 }\n\
               let lambdaMappedValue = match lambdaMapped.get(1) { case Some(value) => value; case None => -1 }\n\
               let parsedManyValue = match parsedMany.get(0) { case Some(Some(value)) => value; case _ => -1 }\n\
               let composedMappedValue = match composedMapped.get(0) { case Some(value) => value; case None => -1 }\n\
               let countedValue = match countedValues.get(1) { case Some(value) => value; case None => -1 }\n\
               let filteredValue = match filtered.get(0) { case Some(value) => value; case None => -1 }\n\
               let optionMappedValue = match optionMapped { case Some(value) => value; case None => -1 }\n\
               let optionFlatValue = match optionFlat { case Some(value) => value; case None => -1 }\n\
               let resultMappedValue = match resultMapped { case Ok(value) => value; case Err(_) => -1 }\n\
               let resultFlatValue = match resultFlat { case Ok(value) => value; case Err(_) => -1 }\n\
               let fallbackValue = match fallback { case Ok(_) => -1; case Err(error) => if error == \"missing\" { 1 } else { -1 } }\n\
               let parsedValue = match parsed { case Some(value) => value; case None => -1 }\n\
               let fetchedValue = match fetched { case Some(value) => value; case None => -1 }\n\
               let sortedValue = match sorted.get(0) { case Some(value) => value; case None => -1 }\n\
               let sortedCopyValue = match sortedCopy.get(2) { case Some(value) => value; case None => -1 }\n\
               let retainedValue = match retained.get(0) { case Some(value) => value; case None => -1 }\n\
               let filteredMapValue = if filteredMap.containsKey(\"b\") && !filteredMap.containsKey(\"a\") { 1 } else { -1 }\n\
               callable.emit(\"callbacks-ready\")\n\
               [shadowMap, shadowLazy, callableField, parsedValue, fetchedValue, mappedValue, lambdaMappedValue, parsedManyValue, composedValue, composedMappedValue, selectedValue, countedValue, callbackCount, filteredValue, reduced, optionMappedValue, optionFlatValue, resultMappedValue, resultFlatValue, fallbackValue, sortedValue, sortedCopyValue, retainedValue, mappedValues.getOr(\"b\", -1), filteredMapValue, keyed.getOr(\"a\", -1), mappedSource.length, mappedSnapshot.length, mappedSnapshot.getOr(\"a\", -1), mappedSnapshot.getOr(\"b\", -1), mappedCalls, filteredSource.length, filteredSnapshot.length]\n\
             }\n",
    );
    let generated = preview_stage1_generated(&host, generated_request())
        .expect("current-source callable member and callback product");
    let ir = generated_ir_payload(&generated.generated_rust)
        .expect("current-source callable member and callback IR payload");
    let runtime_host = Rc::new(topaz_interp::TestHost::new());
    let (value, explicit_main) =
        topaz_stage1_runtime::execute_product_program_with_host_facts_and_input(
            ir,
            &[],
            "",
            None,
            runtime_host.clone(),
        )
        .expect("current-source callable member and callback execution");
    let result = match value {
        Value::Array(values) => values
            .borrow()
            .iter()
            .map(|value| match value {
                Value::Int(value) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>(),
        _ => None,
    };
    assert_eq!(
        (
            generated.status.as_str(),
            explicit_main,
            result,
            runtime_host.stdout(),
        ),
        (
            "completed",
            true,
            Some(vec![
                41, 42, 42, 43, 44, 4, 12, 45, 42, 42, 40, 12, 2, 2, 6, 41, 41, 41, 42, 1, 1, 3, 2,
                20, 1, 2, 3, 2, 10, 20, 2, 3, 1,
            ]),
            vec!["callbacks-ready".to_string()],
        ),
    );
}

#[test]
fn current_source_target_runtime_executes_collection_expressions() {
    let host = InlineLoweringFixtureHost(
        "export function main() -> Array<int> {\n\
               let missing: int | null = null\n\
               let nullObserved = match missing {\n\
                 case null => 1\n\
                 case _ => 0\n\
               }\n\
               let spread = [0, ...[1, 2], 3]\n\
               let stepped = for value in 1..5 by 2 { value * 2 }\n\
               let descending = for value in 5..<0 by -2 { value }\n\
               let literalSet = set { 2, 1, 2 }.toArray()\n\
               let nested = [ for x in [1, 2] for y in [10, 20] if y > 10 => x + y ]\n\
               let setComp = set { for value in [2, 1, 2] => value }.toArray()\n\
               let mapComp = map { for value in [1, 2] => value: value * 10 }\n\
               [nullObserved, spread.length, spread[2], spread[3], stepped.length, stepped[0], stepped[2], descending.length, descending[1], literalSet.length, literalSet[0], literalSet[1], nested.length, nested[0], nested[1], setComp.length, setComp[1], mapComp.getOr(2, -1)]\n\
             }\n",
    );
    let generated = preview_stage1_generated(&host, generated_request())
        .expect("current-source collection expression product");
    let ir = generated_ir_payload(&generated.generated_rust)
        .expect("current-source collection expression IR payload");
    let (value, explicit_main) =
        topaz_stage1_runtime::execute_product_program_with_host_facts_and_input(
            ir,
            &[],
            "",
            None,
            Rc::new(topaz_interp::TestHost::new()),
        )
        .expect("current-source collection expression execution");
    let result = match value {
        Value::Array(values) => values
            .borrow()
            .iter()
            .map(|value| match value {
                Value::Int(value) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>(),
        _ => None,
    };
    assert_eq!(
        (generated.status.as_str(), explicit_main, result),
        (
            "completed",
            true,
            Some(vec![
                1, 4, 2, 3, 3, 2, 10, 3, 3, 2, 2, 1, 2, 21, 22, 2, 1, 20
            ]),
        ),
    );
}

#[test]
fn current_source_target_runtime_schedules_concurrent_arms_and_timeout() {
    let host = InlineLoweringFixtureHost(
        "function spin() -> int {\n\
               let mut value = 0\n\
               while true { value += 1 }\n\
               value\n\
             }\n\
             export function main() -> int {\n\
               let joined = concurrent {\n\
                 answer: 40 + 2\n\
                 count: [1, 2].length\n\
               }\n\
               print(\"{joined.answer}:{joined.count}\")\n\
               let timed = concurrent(timeout: 5ms) {\n\
                 stuck: spin()\n\
               } else {\n\
                 { stuck: -1 }\n\
               }\n\
               print(\"{timed.stuck}\")\n\
               concurrent {\n\
                 stuck: spin()\n\
                 boom: 1 / 0\n\
               }\n\
               0\n\
             }\n",
    );
    let generated = preview_stage1_generated(&host, generated_request())
        .expect("current-source concurrent product");
    let ir = generated_ir_payload(&generated.generated_rust)
        .expect("current-source concurrent IR payload");
    let runtime_host = Rc::new(topaz_interp::TestHost::new());
    runtime_host.set_tick_per_poll(10);
    let error = topaz_stage1_runtime::execute_product_program_with_host_facts_and_input(
        ir,
        &[],
        "",
        None,
        runtime_host.clone(),
    )
    .expect_err("later concurrent arm fault");
    let code = decode_self_product_runtime_diagnostic(&error).map(|error| error.code);
    assert_eq!(
        (generated.status.as_str(), code, runtime_host.stdout(),),
        (
            "completed",
            Some(topaz_value::codes::FAULT_DIV_ZERO),
            vec!["42:2".to_string(), "-1".to_string()],
        ),
    );
}

#[test]
fn current_source_target_runtime_cleans_using_on_structured_exit_not_fault_abort() {
    let return_source = InlineLoweringFixtureHost(
        "let mut normalLater: () -> string = () => \"unset\"\n\
             let mut later: () -> string = () => \"unset\"\n\
             let mut propagatedLater: () -> string = () => \"unset\"\n\
             function fail() -> Result<int, string> { Err(\"stop\") }\n\
             function normal() -> Result<int, string> {\n\
               using file = open(\"fixture.txt\")? {\n\
                 normalLater = () => match file.read() {\n\
                   case Ok(text) => text\n\
                   case Err(error) => error\n\
                 }\n\
                 defer print(match file.read() {\n\
                   case Ok(text) => \"normal:{text}\"\n\
                   case Err(error) => \"normal:{error}\"\n\
                 })\n\
                 99\n\
               }\n\
               Ok(5)\n\
             }\n\
             function scenario() -> Result<int, string> {\n\
               using file = open(\"fixture.txt\")? {\n\
                 later = () => match file.read() {\n\
                   case Ok(text) => text\n\
                   case Err(error) => error\n\
                 }\n\
                 defer print(match file.read() {\n\
                   case Ok(text) => \"defer:{text}\"\n\
                   case Err(error) => \"defer:{error}\"\n\
                 })\n\
                 print(\"body\")\n\
                 return Ok(7)\n\
               }\n\
               Ok(0)\n\
             }\n\
             function propagated() -> Result<int, string> {\n\
               using file = open(\"fixture.txt\")? {\n\
                 propagatedLater = () => match file.read() {\n\
                   case Ok(text) => text\n\
                   case Err(error) => error\n\
                 }\n\
                 defer print(match file.read() {\n\
                   case Ok(text) => \"propagate:{text}\"\n\
                   case Err(error) => \"propagate:{error}\"\n\
                 })\n\
                 fail()?\n\
               }\n\
               Ok(0)\n\
             }\n\
             export function main() -> string {\n\
               let normal = match normal() {\n\
                 case Ok(value) => \"{value}:{normalLater()}\"\n\
                 case Err(error) => error\n\
               }\n\
               let returned = match scenario() {\n\
                 case Ok(value) => \"{value}:{later()}\"\n\
                 case Err(error) => error\n\
               }\n\
               let propagated = match propagated() {\n\
                 case Ok(value) => \"{value}\"\n\
                 case Err(error) => \"{error}:{propagatedLater()}\"\n\
               }\n\
               \"{normal}|{returned}|{propagated}\"\n\
             }\n",
    );
    let return_generated = preview_stage1_generated(&return_source, generated_request())
        .expect("current-source using return product");
    let return_ir = generated_ir_payload(&return_generated.generated_rust)
        .expect("current-source using return IR payload");
    let return_host = Rc::new(topaz_interp::TestHost::new());
    return_host.add_file("fixture.txt", "payload");
    let (return_value, return_explicit_main) =
        topaz_stage1_runtime::execute_product_program_with_host_facts_and_input(
            return_ir,
            &[],
            "",
            None,
            return_host.clone(),
        )
        .expect("current-source using return execution");
    let return_value = match return_value {
        Value::Str(value) => Some(value.to_string()),
        _ => None,
    };
    let return_closed = [1, 2, 3].map(|handle| {
        topaz_value::Host::read(return_host.as_ref(), topaz_value::ResourceId(handle)).err()
    });

    let fault_source = InlineLoweringFixtureHost(
        "export function main() -> Result<int, string> {\n\
               using file = open(\"fixture.txt\")? {\n\
                 defer print(match file.read() {\n\
                   case Ok(text) => \"defer:{text}\"\n\
                   case Err(error) => \"defer:{error}\"\n\
                 })\n\
                 1 / 0\n\
               }\n\
               Ok(0)\n\
             }\n",
    );
    let fault_generated = preview_stage1_generated(&fault_source, generated_request())
        .expect("current-source using fault product");
    let fault_ir = generated_ir_payload(&fault_generated.generated_rust)
        .expect("current-source using fault IR payload");
    let fault_host = Rc::new(topaz_interp::TestHost::new());
    fault_host.add_file("fixture.txt", "payload");
    let fault = topaz_stage1_runtime::execute_product_program_with_host_facts_and_input(
        fault_ir,
        &[],
        "",
        None,
        fault_host.clone(),
    )
    .expect_err("current-source using body fault");
    let fault_code = decode_self_product_runtime_diagnostic(&fault).map(|error| error.code);
    let fault_open = topaz_value::Host::read(fault_host.as_ref(), topaz_value::ResourceId(1)).ok();

    assert_eq!(
        (
            return_generated.status.as_str(),
            return_explicit_main,
            return_value,
            return_host.stdout(),
            return_closed,
            fault_generated.status.as_str(),
            fault_code,
            fault_host.stdout(),
            fault_open,
        ),
        (
            "completed",
            true,
            Some("5:file is closed|7:file is closed|stop:file is closed".to_string()),
            vec![
                "normal:payload".to_string(),
                "body".to_string(),
                "defer:payload".to_string(),
                "propagate:payload".to_string(),
            ],
            [
                Some("file is closed".to_string()),
                Some("file is closed".to_string()),
                Some("file is closed".to_string()),
            ],
            "completed",
            Some(topaz_value::codes::FAULT_DIV_ZERO),
            Vec::<String>::new(),
            Some("payload".to_string()),
        ),
    );
}

#[test]
fn current_source_target_runtime_executes_labeled_value_and_statement_loops() {
    let host = InlineLoweringFixtureHost(
        "export function main() -> Array<int> {\n\
               let mut sum = 0\n\
               for value in [1, 2, 3, 4] {\n\
                 if value == 2 { continue }\n\
                 if value == 4 { break }\n\
                 sum += value\n\
               }\n\
               let direct = loop { break 7 }\n\
               let nested = loop 'outer {\n\
                 let projected = for value in [1] {\n\
                   if value == 1 { break 'outer 9 }\n\
                   0\n\
                 }\n\
                 break 0\n\
               }\n\
               let mut continued = 0\n\
               let mut outer = 0\n\
               loop 'again {\n\
                 outer += 1\n\
                 if outer > 3 { break }\n\
                 let projected = for value in [1] {\n\
                   continued += value\n\
                   continue 'again\n\
                   0\n\
                 }\n\
               }\n\
               let mut captures: Array<() -> int> = []\n\
               let mut index = 0\n\
               while index < 3 {\n\
                 let captured = index\n\
                 captures.push(() => captured)\n\
                 index += 1\n\
               }\n\
               let mut loopCaptures: Array<() -> int> = []\n\
               let mut loopIndex = 0\n\
               loop {\n\
                 let captured = loopIndex\n\
                 loopCaptures.push(() => captured)\n\
                 loopIndex += 1\n\
                 if loopIndex == 3 { break }\n\
               }\n\
               [sum, direct, nested, continued, captures[0](), captures[1](), captures[2](), loopCaptures[0](), loopCaptures[1](), loopCaptures[2]()]\n\
             }\n",
    );
    let generated =
        preview_stage1_generated(&host, generated_request()).expect("current-source loop product");
    let ir =
        generated_ir_payload(&generated.generated_rust).expect("current-source loop IR payload");
    let (value, explicit_main) =
        topaz_stage1_runtime::execute_product_program_with_host_facts_and_input(
            ir,
            &[],
            "",
            None,
            Rc::new(topaz_interp::TestHost::new()),
        )
        .expect("current-source loop execution");
    let result = match value {
        Value::Array(values) => values
            .borrow()
            .iter()
            .map(|value| match value {
                Value::Int(value) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>(),
        _ => None,
    };
    assert_eq!(
        (generated.status.as_str(), explicit_main, result),
        ("completed", true, Some(vec![4, 7, 9, 3, 0, 1, 2, 0, 1, 2]),),
    );
}

#[test]
fn current_source_target_runtime_executes_composite_patterns() {
    let host = InlineLoweringFixtureHost(
        "record User { name: string, age: int }\n\
             record Other { name: string, age: int }\n\
             newtype UserId = int\n\
             function nominal(value: User | Other) -> int {\n\
               match value {\n\
                 case User { name, age: 42 } => name.byteLength()\n\
                 case User { name: _, age } => age\n\
                 case Other { name: _, age } => age\n\
               }\n\
             }\n\
             function unwrap(value: UserId) -> int {\n\
               match value { case UserId(inner) => inner }\n\
             }\n\
             export function main() -> Array<int> {\n\
               let list = match [1, 2, 3, 4] {\n\
                 case [head, ..middle, tail] => [head, middle.length, middle[0], tail]\n\
                 case _ => []\n\
               }\n\
               let bare = match [5, 6, 7] {\n\
                 case [5, .., 7] => 1\n\
                 case _ => 0\n\
               }\n\
               let structural = match { name: \"Ada\", age: 42 } {\n\
                 case { name, age: 42 } => name.byteLength()\n\
                 case _ => 0\n\
               }\n\
               [list[0], list[1], list[2], list[3], bare, structural, nominal(User { name: \"Ada\", age: 42 }), nominal(Other { name: \"Grace\", age: 7 }), unwrap(UserId(9))]\n\
             }\n",
    );
    let generated = preview_stage1_generated(&host, generated_request())
        .expect("current-source composite-pattern product");
    let ir = generated_ir_payload(&generated.generated_rust)
        .expect("current-source composite-pattern IR payload");
    let facts = r#"{
            "schema":"topaz.self-target-adapter-facts/v1",
            "nominals":[
                {"name":"User","identity":"main::User","kind":"record","members":[
                    {"name":"name","arity":1},{"name":"age","arity":1}
                ]},
                {"name":"Other","identity":"main::Other","kind":"record","members":[
                    {"name":"name","arity":1},{"name":"age","arity":1}
                ]},
                {"name":"UserId","identity":"main::UserId","kind":"newtype","members":[]}
            ],
            "operationNominals":[]
        }"#;
    let (value, explicit_main) =
        topaz_stage1_runtime::execute_product_program_with_facts_and_input(
            ir,
            &[],
            "",
            Some(facts),
        )
        .expect("current-source composite-pattern execution");
    let result = match value {
        Value::Array(values) => values
            .borrow()
            .iter()
            .map(|value| match value {
                Value::Int(value) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>(),
        _ => None,
    };
    assert_eq!(
        (generated.status.as_str(), explicit_main, result),
        ("completed", true, Some(vec![1, 2, 2, 4, 1, 3, 3, 7, 9]),),
    );
}

#[test]
fn current_source_target_runtime_tests_typed_patterns() {
    let host = InlineLoweringFixtureHost(
        "record Box<T> { value: T }\n\
             enum Packet<T> { Item(T) }\n\
             newtype Wrapped<T> = T\n\
             function scalar(value: int | string) -> int {\n\
               match value {\n\
                 case text: string => text.byteLength()\n\
                 case number: int => number\n\
               }\n\
             }\n\
             function optional(value: Option<int | string>) -> int {\n\
               match value {\n\
                 case found: Option<int> => 1\n\
                 case _ => 0\n\
               }\n\
             }\n\
             function primitive(value: bool | float) -> int {\n\
               match value {\n\
                 case found: bool => 1\n\
                 case _ => 0\n\
               }\n\
             }\n\
             function unitPattern(value: () | int) -> int {\n\
               match value {\n\
                 case found: () => 1\n\
                 case _ => 0\n\
               }\n\
             }\n\
             function resultType(value: Result<int | string, string>) -> int {\n\
               match value {\n\
                 case found: Result<int, string> => 1\n\
                 case _ => 0\n\
               }\n\
             }\n\
             function structural(value: { item: int } | { item: string }) -> int {\n\
               match value {\n\
                 case found: { item: int } => 1\n\
                 case _ => 0\n\
               }\n\
             }\n\
             function nominal(value: Box<int> | Box<string>) -> int {\n\
               match value {\n\
                 case found: Box<int> => 1\n\
                 case _ => 0\n\
               }\n\
             }\n\
             function enumNominal(value: Packet<int> | Packet<string>) -> int {\n\
               match value {\n\
                 case found: Packet<int> => 1\n\
                 case _ => 0\n\
               }\n\
             }\n\
             function newtypeNominal(value: Wrapped<int> | Wrapped<string>) -> int {\n\
               match value {\n\
                 case found: Wrapped<int> => 1\n\
                 case _ => 0\n\
               }\n\
             }\n\
             function collection(value: Array<int> | Array<string>) -> int {\n\
               match value {\n\
                 case found: Array<int> => 1\n\
                 case _ => 0\n\
               }\n\
             }\n\
             function setType(value: Set<int> | Set<string>) -> int {\n\
               match value {\n\
                 case found: Set<int> => 1\n\
                 case _ => 0\n\
               }\n\
             }\n\
             function mapType(value: Map<string, int> | Map<string, string>) -> int {\n\
               match value {\n\
                 case found: Map<string, int> => 1\n\
                 case _ => 0\n\
               }\n\
             }\n\
             function literal(value: 7 | 8) -> int {\n\
               match value {\n\
                 case found: 7 => 1\n\
                 case _ => 0\n\
               }\n\
             }\n\
             function unary(value: int) -> int { value }\n\
             function binary(left: string, right: string) -> string { left + right }\n\
             function callable<T>(value: T) -> int {\n\
               match value {\n\
                 case found: (int) -> int => 1\n\
                 case _ => 0\n\
               }\n\
             }\n\
             export function main() -> Array<int> {\n\
               let intBox: Box<int> = Box { value: 7 }\n\
               let stringBox: Box<string> = Box { value: \"Ada\" }\n\
               let intPacket: Packet<int> = Packet.Item(7)\n\
               let stringPacket: Packet<string> = Packet.Item(\"Ada\")\n\
               let intWrapped: Wrapped<int> = Wrapped(7)\n\
               let stringWrapped: Wrapped<string> = Wrapped(\"Ada\")\n\
               let intResult: Result<int | string, string> = Ok(7)\n\
               let stringResult: Result<int | string, string> = Ok(\"Ada\")\n\
               let mut intMap: Map<string, int> = Map.new()\n\
               intMap.insert(\"item\", 7)\n\
               let mut stringMap: Map<string, string> = Map.new()\n\
               stringMap.insert(\"item\", \"Ada\")\n\
               [scalar(7), scalar(\"Ada\"), optional(Some(7)), optional(Some(\"Ada\")), primitive(true), primitive(1.5), unitPattern(()), unitPattern(7), resultType(intResult), resultType(stringResult), structural({ item: 7 }), structural({ item: \"Ada\" }), nominal(intBox), nominal(stringBox), enumNominal(intPacket), enumNominal(stringPacket), newtypeNominal(intWrapped), newtypeNominal(stringWrapped), collection([7]), collection([\"Ada\"]), setType(Set.of(7)), setType(Set.of(\"Ada\")), mapType(intMap), mapType(stringMap), literal(7), literal(8), callable(unary), callable(binary)]\n\
             }\n",
    );
    let generated = preview_stage1_generated(&host, generated_request())
        .expect("current-source typed-pattern product");
    let ir = generated_ir_payload(&generated.generated_rust)
        .expect("current-source typed-pattern IR payload");
    let typed = decode_stage1_typed_from_generated(&generated)
        .expect("current-source typed-pattern semantic facts");
    let lowered = decode_stage1_lowering_from_generated(&generated)
        .expect("current-source typed-pattern lowering facts");
    let entry_module = typed
        .resolved
        .modules
        .iter()
        .find(|module| module.entry)
        .expect("current-source typed-pattern entry module")
        .identity
        .clone();
    let facts = encode_target_adapter_facts(&SelfTargetAdapterFacts {
        schema: SELF_TARGET_ADAPTER_FACTS_SCHEMA,
        producer: CompilerProducer::Stage1.identity(),
        source_set_id: generated.provenance_source_set_id.clone(),
        result_id: stage1_sha256(&generated.response),
        entry_module,
        exports: Vec::new(),
        nominals: project_target_nominal_facts(&typed, &lowered.operations)
            .expect("current-source typed-pattern nominal facts"),
        operation_nominals: Vec::new(),
        runtime_requirements: Vec::new(),
    });
    let (value, explicit_main) =
        topaz_stage1_runtime::execute_product_program_with_facts_and_input(
            ir,
            &[],
            "",
            Some(&facts),
        )
        .expect("current-source typed-pattern execution");
    let result = match value {
        Value::Array(values) => values
            .borrow()
            .iter()
            .map(|value| match value {
                Value::Int(value) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>(),
        _ => None,
    };
    assert_eq!(
        (generated.status.as_str(), explicit_main, result),
        (
            "completed",
            true,
            Some(vec![
                7, 3, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0,
            ]),
        ),
    );
}

#[test]
fn linked_stage2_main_receives_args_and_stdin() {
    let host = InlineLoweringFixtureHost(
        "export function main(args: Array<string>, stdin: string) -> string {\n\
               \"{args.length}:{stdin}\"\n\
             }\n",
    );
    let product = preview_linked_stage2_compilation_product(
        &host,
        generated_request(),
        CompilationProfile::None,
    )
    .expect("two-parameter self product");
    let (value, explicit_main) = execute_self_compilation_product_with_input(
        &product,
        &["one".to_string(), "two".to_string()],
        "payload",
    )
    .expect("two-parameter self execution");
    assert!(explicit_main);
    assert!(matches!(value, Value::Str(value) if value.as_ref() == "2:payload"));
}

#[test]
fn linked_stage2_contextual_enum_variants_are_not_lowered_as_bindings() {
    let host = InlineLoweringFixtureHost(
        "enum Token { Plus, Star }\n\
             function apply(token: Token) -> int {\n\
               match token {\n\
                 case Plus => 1\n\
                 case Star => 2\n\
               }\n\
             }\n\
             export function main() -> int { apply(Token.Star) }\n",
    );
    let product = preview_linked_stage2_compilation_product(
        &host,
        generated_request(),
        CompilationProfile::None,
    )
    .expect("contextual enum product");
    let contextual = product
        .lowered
        .operations
        .iter()
        .filter(|operation| {
            operation.kind == "pattern/constructor"
                && matches!(operation.detail.as_str(), "Plus" | "Star")
        })
        .collect::<Vec<_>>();
    assert_eq!(contextual.len(), 2);
    assert!(
        contextual
            .iter()
            .all(|operation| operation.binding_name.is_empty())
    );
    let (value, explicit_main) =
        execute_self_compilation_product(&product, &[]).expect("contextual enum execution");
    assert!(explicit_main);
    assert!(matches!(value, Value::Int(2)));
}

#[test]
fn linked_stage2_enum_or_pattern_variants_are_not_bindings() {
    let host = InlineLoweringFixtureHost(
        "enum Token { Plus, Star }\n\
             function apply(token: Token) -> int {\n\
               match token {\n\
                 case Plus | Star => 1\n\
               }\n\
             }\n\
             export function main() -> int { apply(Token.Star) }\n",
    );
    let product = preview_linked_stage2_compilation_product(
        &host,
        generated_request(),
        CompilationProfile::None,
    )
    .expect("contextual enum or-pattern product");
    assert!(product.typed.diagnostics.is_empty());
    let (value, explicit_main) =
        execute_self_compilation_product(&product, &[]).expect("enum or-pattern execution");
    assert!(explicit_main);
    assert!(matches!(value, Value::Int(1)));
}

#[test]
fn linked_stage2_match_guard_selects_the_case_body() {
    let host = InlineLoweringFixtureHost(
        "function pick(value: int) -> int {\n\
               match Some(value) {\n\
                 case Some(item) if item > 0 => item\n\
                 case _ => 0\n\
               }\n\
             }\n\
             export function main() -> string { \"{pick(7)}:{pick(-1)}\" }\n",
    );
    let product = preview_linked_stage2_compilation_product(
        &host,
        generated_request(),
        CompilationProfile::None,
    )
    .expect("guarded match product");
    let guarded_match = product
        .lowered
        .operations
        .iter()
        .find(|operation| operation.kind == "expression/match")
        .expect("guarded match operation");
    assert!(
        guarded_match
            .operand_labels
            .iter()
            .any(|label| label.contains("/guard:"))
    );
    let (value, explicit_main) =
        execute_self_compilation_product(&product, &[]).expect("guarded match execution");
    assert!(explicit_main);
    assert!(matches!(value, Value::Str(value) if value.as_ref() == "7:0"));
}

#[test]
fn linked_stage2_explicit_builtin_type_arguments_are_specialized() {
    let host = InlineLoweringFixtureHost(
        "record Payload { value: int }\n\
             function decode(value: JSONValue) -> Result<Payload, string> {\n\
               JSON.decode<Payload>(value)\n\
             }\n\
             export function main() -> int {\n\
               let parsed = match JSON.parse(\"\\{\\\"value\\\":1\\}\") {\n\
                 case Ok(value) => value\n\
                 case Err(_) => return 0\n\
               }\n\
               let keys = match parsed.keys() {\n\
                 case Some(value) => value\n\
                 case None => []\n\
               }\n\
               let decoded = match decode(parsed) {\n\
                 case Ok(value) => value\n\
                 case Err(_) => return 0\n\
               }\n\
               let mut values: Set<string> = Set.of<string>()\n\
               values.add(\"ok\")\n\
               decoded.value + keys.length + values.length\n\
             }\n",
    );
    let product = preview_linked_stage2_compilation_product(
        &host,
        generated_request(),
        CompilationProfile::None,
    )
    .expect("explicit builtin type-argument product");
    assert_eq!(product.status(), "completed");
    assert!(product.typed.diagnostics.is_empty());
    let (value, explicit_main) =
        execute_self_compilation_product(&product, &[]).expect("explicit builtin execution");
    assert!(explicit_main);
    assert!(matches!(value, Value::Int(3)));
}

#[test]
fn linked_stage2_lit_static_standard_library_leaves_execute() {
    let host = InlineLoweringFixtureHost(
        "record Bucket { items: Array<int> }\n\
             function pushThroughFrame(frames: Array<Bucket>, item: int) -> () {\n\
               let frame = frames[0]\n\
               let mut items: Array<int> = frame.items\n\
               items.push(item)\n\
             }\n\
             function appendThroughAlias(bucket: Bucket, item: int) -> int {\n\
               let mut items: Array<int> = bucket.items\n\
               loop {\n\
                 items.push(item)\n\
                 break\n\
               }\n\
               items.length\n\
             }\n\
             function directFailure() -> Result<int, string> {\n\
               Err(\"expected\")\n\
             }\n\
             function propagatedFailure() -> Result<int, string> {\n\
               let value = directFailure()?\n\
               Ok(value)\n\
             }\n\
             export function main() -> int {\n\
               let decoded = match Bytes.fromBase64(\"b2s=\") {\n\
                 case Ok(value) => value\n\
                 case Err(_) => return 0\n\
               }\n\
               let copied = match Bytes.fromArray(decoded.toArray()) {\n\
                 case Ok(value) => value\n\
                 case Err(_) => return 0\n\
               }\n\
               let number = match Math.parseFloat(\"1.5\") {\n\
                 case Ok(value) => value\n\
                 case Err(_) => return 0\n\
               }\n\
               let letter = fromCodePoint(65) ?? \"\"\n\
               let mut loops = 0\n\
               loop {\n\
                 loops += 1\n\
                 if loops == 2 { break }\n\
               }\n\
               let mut scratch: Array<int> = []\n\
               scratch.push(1)\n\
               let mut frames = [Bucket { items: [] }]\n\
               pushThroughFrame(frames, 1)\n\
               let frameMutation = frames[0].items.length\n\
               let propagation = match propagatedFailure() {\n\
                 case Err(_) => 1\n\
                 case Ok(_) => 0\n\
               }\n\
               if copied.decodeUtf8() == Ok(\"ok\") &&\n\
                 Hash.sha256(copied).length() == 32 &&\n\
                 Math.abs(number) == 1.5 && Math.isFinite(number) && letter == \"A\" {\n\
                 loops + scratch.length + appendThroughAlias(Bucket { items: [] }, 1) + propagation + frameMutation\n\
               } else {\n\
                 0\n\
               }\n\
             }\n",
    );
    let product = preview_linked_stage2_compilation_product(
        &host,
        generated_request(),
        CompilationProfile::None,
    )
    .expect("LIT static standard-library leaf product");
    assert_eq!(
        product.status(),
        "completed",
        "LIT diagnostics: {:?}; resolver diagnostics: {:?}; unsupported: {:?}",
        product.typed.diagnostics,
        product.typed.resolved.diagnostics,
        product.lowered.unsupported
    );
    assert!(product.typed.diagnostics.is_empty());
    let (value, explicit_main) =
        execute_self_compilation_product(&product, &[]).expect("LIT static leaf execution");
    assert!(explicit_main);
    assert!(matches!(value, Value::Int(6)));
}

#[test]
fn linked_stage2_reader_pair_construction_preserves_items() {
    let host = InlineLoweringFixtureHost(
        "enum MiniKind {\n\
               MiniNil,\n\
               MiniSymbol(string),\n\
               MiniPair(int, int),\n\
             }\n\
             record MiniDatum { kind: MiniKind }\n\
             record MiniFrame { items: Array<int> }\n\
             function addDatum(datums: Array<MiniDatum>, kind: MiniKind) -> int {\n\
               let mut arena = datums\n\
               let id = arena.length\n\
               arena.push(MiniDatum { kind: kind })\n\
               id\n\
             }\n\
             function deliver(\n\
               frames: Array<MiniFrame>,\n\
               active: Array<int>,\n\
               roots: Array<int>,\n\
               datum: int,\n\
             ) -> () {\n\
               let mut stack = frames\n\
               let mut length = active\n\
               let mut output = roots\n\
               if length[0] == 0 {\n\
                 output.push(datum)\n\
                 return\n\
               }\n\
               let frame = stack[length[0] - 1]\n\
               let mut items = frame.items\n\
               items.push(datum)\n\
             }\n\
             function finish(datums: Array<MiniDatum>, frame: MiniFrame) -> int {\n\
               let mut tail = addDatum(datums, MiniKind.MiniNil)\n\
               let mut index = frame.items.length - 1\n\
               while index >= 0 {\n\
                 tail = addDatum(datums, MiniKind.MiniPair(frame.items[index], tail))\n\
                 index -= 1\n\
               }\n\
               tail\n\
             }\n\
             function countItems(datums: Array<MiniDatum>, root: int) -> int {\n\
               let mut current = root\n\
               let mut count = 0\n\
               loop {\n\
                 match datums[current].kind {\n\
                   case MiniNil => return count\n\
                   case MiniPair(_, tail) => {\n\
                     count += 1\n\
                     current = tail\n\
                   }\n\
                   case MiniSymbol(_) => return -1\n\
                 }\n\
               }\n\
               count\n\
             }\n\
             export function main() -> int {\n\
               let mut datums: Array<MiniDatum> = []\n\
               let mut frames = [MiniFrame { items: [] }]\n\
               let mut active = [1]\n\
               let mut roots: Array<int> = []\n\
               deliver(frames, active, roots, addDatum(datums, MiniKind.MiniSymbol(\"+\")))\n\
               deliver(frames, active, roots, addDatum(datums, MiniKind.MiniSymbol(\"20\")))\n\
               deliver(frames, active, roots, addDatum(datums, MiniKind.MiniSymbol(\"22\")))\n\
               active[0] -= 1\n\
               let root = finish(datums, frames[0])\n\
               deliver(frames, active, roots, root)\n\
               countItems(datums, roots[0])\n\
             }\n",
    );
    let product = preview_linked_stage2_compilation_product(
        &host,
        generated_request(),
        CompilationProfile::None,
    )
    .expect("reader pair-construction product");
    assert_eq!(
        product.status(),
        "completed",
        "reader diagnostics: {:?}; resolver diagnostics: {:?}; unsupported: {:?}",
        product.typed.diagnostics,
        product.typed.resolved.diagnostics,
        product.lowered.unsupported
    );
    assert!(product.typed.diagnostics.is_empty());
    let (value, explicit_main) =
        execute_self_compilation_product(&product, &[]).expect("reader pair execution");
    assert!(explicit_main);
    assert!(
        matches!(value, Value::Int(3)),
        "reader pair count: {value:?}"
    );
}

#[test]
fn linked_stage2_profile_and_syntax_diagnostics_are_owned_by_c2() {
    let host = InlineLoweringFixtureHost("let checked = assert(true)\n");
    let result = preview_linked_stage2_compilation_product(
        &host,
        generated_request(),
        CompilationProfile::AgentPack,
    )
    .expect("profiled self product");
    assert_eq!(result.status(), "rejected");
    assert!(result.lowered.modules.is_empty());
    assert!(result.lowered.operations.is_empty());
    assert!(result.generated_rust().is_empty());
    assert_eq!(result.typed.diagnostics.len(), 1);
    assert!(
        !result.typed.nodes.is_empty(),
        "a profile-only rejection must retain typed nodes"
    );
    assert!(
        !result.typed.calls.is_empty(),
        "a profile-only rejection must retain typed calls"
    );
    assert_eq!(
        result.typed.diagnostics[0].profile_rule.as_deref(),
        Some("agent-pack/no-assert")
    );
    assert_eq!(
        result.typed.diagnostics[0].message,
        "`assert` is not allowed by profile `agent-pack`"
    );
    let manifest =
        encode_self_compilation_product_manifest(&result).expect("rejected product manifest");
    validate_self_compilation_product_manifest(&manifest).expect("valid rejected product manifest");

    let syntax = preview_linked_stage2_compilation_product(
        &InlineLoweringFixtureHost("let value = (1, 2)\n"),
        generated_request(),
        CompilationProfile::None,
    )
    .expect("sealed-image syntax rejection");
    let diagnostics = syntax
        .typed
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str(),
                diagnostic.message.as_str(),
                diagnostic.lo,
                diagnostic.hi,
                diagnostic.notes.len(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        (
            syntax.status(),
            syntax.generated_rust().is_empty(),
            syntax.lowered.modules.is_empty(),
            syntax.lowered.operations.is_empty(),
            diagnostics,
        ),
        (
            "rejected",
            true,
            true,
            true,
            vec![(
                "TPZ2001",
                "parentheses group a single expression; `(a, b)` comma lists are not Topaz syntax",
                14,
                15,
                0,
            )],
        )
    );
}

#[test]
fn linked_stage2_profile_inventory_covers_all_current_profiles() {
    for (profile, source, expected_rule) in [
        (
            CompilationProfile::AgentPack,
            "let checked = assert(true)\n",
            "agent-pack/no-assert",
        ),
        (
            CompilationProfile::TestProfile,
            "let checked = Test.assertEq(1, 1)\n",
            "test-profile/no-test-framework",
        ),
        (
            CompilationProfile::Bootstrap,
            "let fraction = 1.5\n",
            "bootstrap/no-float",
        ),
    ] {
        let host = InlineLoweringFixtureHost(source);
        let result = preview_linked_stage2_profiled_lowered(&host, lowering_request(), profile)
            .expect("profiled self result");
        assert_eq!(result.status, "rejected");
        let typed = decode_stage1_typed_preview(&result.request, &result.front_end, result.rounds)
            .expect("profiled typed result");
        assert!(
            typed
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.profile_rule.as_deref() == Some(expected_rule)),
            "missing {expected_rule}: {:#?}",
            typed
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.profile_rule.as_deref())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn stage1_source_profile_prefers_resolved_values_over_builtin_spelling() {
    let session = FrontEndSession::new().expect("checked embedded compiler source");
    for (profile, source) in [
        (
            CompilationProfile::AgentPack,
            "let Test = { assert: (value: bool) => () }\nlet checked = Test.assert(true)\n",
        ),
        (
            CompilationProfile::Bootstrap,
            "let FS = { readText: (path: string) => path }\nlet text = FS.readText(\"ok\")\n",
        ),
    ] {
        let host = InlineLoweringFixtureHost(source);
        let result = preview_stage1_lowered_by(
            |encoded| session.invoke_stage1(encoded),
            &host,
            lowering_request(),
            false,
            CompilerProducer::Stage1,
            profile,
        )
        .expect("profiled self result");
        let typed = decode_stage1_typed_preview(&result.request, &result.front_end, result.rounds)
            .expect("profiled typed result");
        assert_eq!(
            result.status, "completed",
            "diagnostics={:#?}; unsupported={:#?}",
            typed.diagnostics, result.unsupported
        );
        assert!(typed.diagnostics.is_empty(), "{:#?}", typed.diagnostics);
    }
}

#[test]
fn stage1_source_profile_reports_policy_findings_with_syntax_errors() {
    let source = "let inc = (x: int) => x + 1\n\
                      let twice = (x: int) => x * 2\n\
                      let composed = inc >> twice\n\
                      let broken = )\n";
    let session = FrontEndSession::new().expect("checked embedded compiler source");
    let host = InlineLoweringFixtureHost(source);
    let result = preview_stage1_lowered_by(
        |encoded| session.invoke_stage1(encoded),
        &host,
        lowering_request(),
        false,
        CompilerProducer::Stage1,
        CompilationProfile::AgentPack,
    )
    .expect("profiled self result");
    let typed = decode_stage1_typed_preview(&result.request, &result.front_end, result.rounds)
        .expect("profiled typed result");
    assert_eq!(result.status, "rejected");
    assert!(
        typed
            .resolved
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "TPZ2001")
    );
    let composition = typed
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.profile_rule.as_deref() == Some("agent-pack/no-composition"))
        .expect("composition profile finding alongside syntax errors");
    assert_eq!(
        &source[composition.lo as usize..composition.hi as usize],
        ">>"
    );
}

#[test]
fn linked_stage2_nested_composition_reports_each_operator_span() {
    let host = InlineLoweringFixtureHost(
        "let inc = (x: int) => x + 1\n\
             let twice = (x: int) => x * 2\n\
             let dec = (x: int) => x - 1\n\
             let composed = inc >> twice >> dec\n",
    );
    let result = preview_linked_stage2_profiled_lowered(
        &host,
        lowering_request(),
        CompilationProfile::AgentPack,
    )
    .expect("nested composition profile result");
    let typed = decode_stage1_typed_preview(&result.request, &result.front_end, result.rounds)
        .expect("nested composition typed result");
    let findings = typed
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.profile_rule.as_deref() == Some("agent-pack/no-composition")
        })
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 2);
    assert_ne!(
        (findings[0].lo, findings[0].hi),
        (findings[1].lo, findings[1].hi)
    );
}
