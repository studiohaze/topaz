use super::*;

#[test]
fn emits_direct_lambda_calls_with_immutable_capture() {
    let generated = emit_source(
        r#"
function main() -> int {
    let x = 1
    let a = ((y) => y + 1)(4)
    let b = ((y) => x + y)(2)
    a + b
}
main()
"#,
    );
    assert!(generated.contains("lambda _t_79"), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("direct lambda Python gate failed: {e}"));
}

#[test]
fn emits_lambda_valued_local_calls_with_immutable_capture() {
    let generated = emit_source(
        r#"
function main() -> int {
    let x = 1
    let inc = (y) => y + 1
    let addX = (y) => x + y
    inc(4) + addX(2)
}
main()
"#,
    );
    assert!(
        generated.contains("_t_696e63 = (lambda _t_79"),
        "{generated}"
    );
    assert!(
        generated.contains("_t_696e63(4)"),
        "lambda-valued local call should call the local binding: {generated}"
    );
    assert!(
        generated.contains("_t_61646458(2)"),
        "capturing lambda-valued local call should call the local binding: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("lambda-valued local Python gate failed: {e}"));
}

#[test]
fn emits_bound_callable_pipe_and_hof_callbacks() {
    let generated = emit_source(
        r#"
function inc(x: int) -> int {
    x + 1
}
function add(a: int, b: int) -> int {
    a + b
}
function main() -> int {
    let xs = [1, 2, 3]
    let topInc = inc
    let topAdd = add
    let localDouble = (x: int) => x * 2
    let localKeep = (x: int) => x > 2
    let directTop = 4 |> topInc
    let namedTop = 5 |> topAdd(b: 7, a: _)
    let directLocal = 6 |> localDouble
    let mappedTop = xs.map(topInc)
    let filteredLocal = xs.filter(localKeep)
    let reducedTop = 0 |> xs.reduce(topAdd)
    directTop + namedTop + directLocal + mappedTop[0] + filteredLocal.length + reducedTop
}
main()
"#,
    );
    assert!(
        generated.contains("_t_746f70496e63 = tpz_host_callable(_t_696e63, host"),
        "top-level function values should be host-adapted before local binding: {generated}"
    );
    assert!(
        generated.contains("_t_746f70496e63(__tpz_piped)"),
        "bound top-level function should be callable as a pipe target: {generated}"
    );
    assert!(
        generated.contains("_t_746f70416464(_t_62=7, _t_61=__tpz_piped)"),
        "bound top-level function should preserve named pipe placeholder binding: {generated}"
    );
    assert!(
        generated.contains("_t_6c6f63616c446f75626c65(__tpz_piped)"),
        "bound local lambda should be callable as a pipe target: {generated}"
    );
    assert!(
        generated.contains("tpz_array_map(_t_7873, _t_746f70496e63,"),
        "bound top-level function should adapt as a receiver HOF callback: {generated}"
    );
    assert!(
        generated.contains("tpz_array_filter(_t_7873, _t_6c6f63616c4b656570,"),
        "bound local lambda should adapt as a receiver HOF callback: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("bound callable pipe/HOF Python gate failed: {e}"));
}

#[test]
fn emits_function_composition() {
    let generated = emit_source(
        r#"
function inc(x: int) -> int {
    x + 1
}
function dbl(x: int) -> int {
    x * 2
}
function main() -> int {
    let top = inc >> dbl
    let local = ((x) => x + 3) >> ((x) => x * 4)
    let nested = top >> ((x) => x - 1)
    top(5) + local(2) + nested(1)
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_compose(tpz_host_callable(_t_696e63, host"),
        "top-level compose operand should be host-adapted: {generated}"
    );
    assert!(
        generated.contains("tpz_compose((lambda _t_78: tpz_add(_t_78, 3,"),
        "lambda compose operand should lower directly: {generated}"
    );
    assert!(
        generated.contains("_t_746f70(5)") && generated.contains("_t_6c6f63616c(2)"),
        "composed local bindings should be callable: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("function composition Python gate failed: {e}"));
}

#[test]
fn emits_free_map_hof_with_lambda_and_function_callbacks() {
    let generated = emit_source(
        r#"
function double(x: int) -> int {
    x * 2
}
function main() -> int {
    let xs = map([1, 2], (x) => x + 1)
    let ys = map([3], double)
    xs[0] + xs[1] + ys[0]
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_array_map([1, 2], (lambda _t_78"),
        "inline lambda callback should lower through tpz_array_map: {generated}"
    );
    assert!(
        generated
            .contains("tpz_array_map([3], (lambda __tpz_cb_0: _t_646f75626c65(host, __tpz_cb_0))"),
        "top-level function callback should adapt host argument: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("free map HOF Python gate failed: {e}"));
}

#[test]
fn emits_receiver_array_map_hof_with_lambda_and_function_callbacks() {
    let generated = emit_source(
        r#"
function double(x: int) -> int {
    x * 2
}
function main() -> int {
    let xs = [1, 2]
    let ys = xs.map((x) => x + 1)
    let zs = xs.map(double)
    ys[0] + ys[1] + zs[0] + zs[1]
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_array_map(_t_7873, (lambda _t_78"),
        "receiver lambda callback should lower through tpz_array_map: {generated}"
    );
    assert!(
        generated.contains(
            "tpz_array_map(_t_7873, (lambda __tpz_cb_0: _t_646f75626c65(host, __tpz_cb_0))"
        ),
        "receiver function callback should adapt host argument: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("receiver map HOF Python gate failed: {e}"));
}

#[test]
fn emits_array_filter_hofs_with_lambda_and_function_callbacks() {
    let generated = emit_source(
        r#"
function big(x: int) -> bool {
    x > 1
}
function main() -> int {
    let xs = [1, 2, 3]
    let a = filter(xs, (x) => x > 1)
    let b = xs.filter(big)
    a.length + b.length
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_array_filter(_t_7873, (lambda _t_78"),
        "free filter lambda callback should lower through tpz_array_filter: {generated}"
    );
    assert!(
        generated
            .contains("tpz_array_filter(_t_7873, (lambda __tpz_cb_0: _t_626967(host, __tpz_cb_0))"),
        "receiver filter function callback should adapt host argument: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("array filter HOF Python gate failed: {e}"));
}

#[test]
fn emits_array_reduce_hofs_with_lambda_and_function_callbacks() {
    let generated = emit_source(
        r#"
function add(acc: int, x: int) -> int {
    acc + x
}
function main() -> int {
    let xs = [1, 2, 3]
    let a = reduce(xs, 0, (acc, x) => acc + x)
    let b = xs.reduce(10, add)
    a + b
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_array_reduce(_t_7873, 0, (lambda _t_616363, _t_78"),
        "free reduce lambda callback should lower through tpz_array_reduce: {generated}"
    );
    assert!(
        generated.contains("tpz_array_reduce(_t_7873, 10, (lambda __tpz_cb_0, __tpz_cb_1: _t_616464(host, __tpz_cb_0, __tpz_cb_1))"),
        "receiver reduce function callback should adapt host and two args: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("array reduce HOF Python gate failed: {e}"));
}

#[test]
fn emits_array_sorted_by_hofs_with_lambda_and_function_callbacks() {
    let generated = emit_source(
        r#"
function neg(x: int) -> int {
    0 - x
}
function main() -> int {
    let xs = [3, 1, 2]
    let desc = xs.sortedBy((x) => 0 - x)
    let desc2 = xs.sortedBy(neg)
    desc[0] + desc2[2]
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_array_sorted_by(_t_7873, (lambda _t_78"),
        "receiver sortedBy lambda callback should lower through tpz_array_sorted_by: {generated}"
    );
    assert!(
        generated.contains(
            "tpz_array_sorted_by(_t_7873, (lambda __tpz_cb_0: _t_6e6567(host, __tpz_cb_0))"
        ),
        "receiver sortedBy function callback should adapt host argument: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("array sortedBy HOF Python gate failed: {e}"));
}

#[test]
fn emits_array_sort_by_and_retain_hofs_with_lambda_and_function_callbacks() {
    let generated = emit_source(
        r#"
function neg(x: int) -> int {
    0 - x
}
function large(x: int) -> bool {
    x > 2
}
function main() -> int {
    let mut xs = [3, 1, 2]
    xs.sortBy((x) => 0 - x)
    let mut ys = [3, 1, 2]
    ys.sortBy(neg)
    let mut a = [1, 2, 3, 4]
    a.retain((x) => x % 2 == 0)
    let mut b = [1, 2, 3, 4]
    b.retain(large)
    xs[0] + ys[2] + a.length + b.length
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_array_sort_by(_t_7873, (lambda _t_78"),
        "sortBy lambda callback should lower through tpz_array_sort_by: {generated}"
    );
    assert!(
        generated.contains(
            "tpz_array_sort_by(_t_7973, (lambda __tpz_cb_0: _t_6e6567(host, __tpz_cb_0))"
        ),
        "sortBy function callback should adapt host argument: {generated}"
    );
    assert!(
        generated.contains("tpz_array_retain(_t_61, (lambda _t_78"),
        "retain lambda callback should lower through tpz_array_retain: {generated}"
    );
    assert!(
        generated.contains(
            "tpz_array_retain(_t_62, (lambda __tpz_cb_0: _t_6c61726765(host, __tpz_cb_0))"
        ),
        "retain function callback should adapt host argument: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("array sortBy/retain HOF Python gate failed: {e}"));
}

#[test]
fn emits_option_and_result_map_hofs() {
    let generated = emit_source(
        r#"
function inc(x: int) -> int {
    x + 1
}
function makeOption() -> Option<int> {
    Some(4)
}
function makeOptionArray() -> Option<Array<int>> {
    Some([1, 2])
}
function makeResult() -> Result<int, string> {
    Ok(5)
}
function main() -> int {
    let opt = Some(1)
    let a = opt.map((x) => x + 1)
    let b = Ok(2).map(inc)
    let c = Err("bad").map(inc)
    let d = makeOption().map(inc)
    let xs = makeOptionArray()
    let e = xs?.map(inc)
    let f = makeOptionArray()?.map(inc)
    let g = makeResult().map(inc)
    let av = match a {
        case Some(n) => n
        case None => 0
    }
    let bv = match b {
        case Ok(n) => n
        case Err(_) => 0
    }
    let cv = match c {
        case Ok(_) => 0
        case Err(_) => 1
    }
    let dv = match d {
        case Some(n) => n
        case None => 0
    }
    let ev = match e {
        case Some(ys) => ys[0] + ys[1]
        case None => 0
    }
    let fv = match f {
        case Some(ys) => ys[0] + ys[1]
        case None => 0
    }
    let gv = match g {
        case Ok(n) => n
        case Err(_) => 0
    }
    av + bv + cv + dv + ev + fv + gv
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_option_map(_t_6f7074, (lambda _t_78"),
        "Option.map lambda callback should lower through tpz_option_map: {generated}"
    );
    assert!(
        generated
            .contains("tpz_result_map(Ok(2), (lambda __tpz_cb_0: _t_696e63(host, __tpz_cb_0))"),
        "Result.map function callback should adapt host argument: {generated}"
    );
    assert!(
        generated.contains(
            "tpz_option_map(_t_6d616b654f7074696f6e(host), (lambda __tpz_cb_0: _t_696e63(host, __tpz_cb_0))"
        ),
        "Option-returning function calls should track receiver shape: {generated}"
    );
    assert!(
        generated.contains(
            "tpz_wrap_optional(tpz_array_map(__tpz_obj.value, (lambda __tpz_cb_0: _t_696e63(host, __tpz_cb_0))"
        ),
        "Option<Array>-returning function calls should track optional receiver inner shape: {generated}"
    );
    assert!(
        generated.contains(
            "tpz_result_map(_t_6d616b65526573756c74(host), (lambda __tpz_cb_0: _t_696e63(host, __tpz_cb_0))"
        ),
        "Result-returning function calls should track receiver shape: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("Option/Result map HOF Python gate failed: {e}"));
}

#[test]
fn emits_option_and_result_flat_map_hofs() {
    let generated = emit_source(
        r#"
function maybeInc(x: int) -> Option<int> {
    Some(x + 1)
}
function okInc(x: int) -> Result<int, string> {
    Ok(x + 1)
}
function main() -> int {
    let a = Some(1).flatMap((x) => Some(x + 1))
    let b = Some(2).flatMap(maybeInc)
    let c = Ok(3).flatMap(okInc)
    let d = Err("bad").flatMap(okInc)
    let av = match a {
        case Some(n) => n
        case None => 0
    }
    let bv = match b {
        case Some(n) => n
        case None => 0
    }
    let cv = match c {
        case Ok(n) => n
        case Err(_) => 0
    }
    let dv = match d {
        case Ok(_) => 0
        case Err(_) => 1
    }
    av + bv + cv + dv
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_option_flat_map(Some(1), (lambda _t_78"),
        "Option.flatMap lambda callback should lower through tpz_option_flat_map: {generated}"
    );
    assert!(
        generated.contains(
            "tpz_result_flat_map(Ok(3), (lambda __tpz_cb_0: _t_6f6b496e63(host, __tpz_cb_0))"
        ),
        "Result.flatMap function callback should adapt host argument: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("Option/Result flatMap HOF Python gate failed: {e}"));
}

#[test]
fn emits_option_ok_or_else_lazy_callback() {
    let generated = emit_source(
        r#"
function late() -> string {
    "late"
}
function fail() -> string {
    let n = 1 / 0
    "bad"
}
function main() -> int {
    let none: Option<int> = None
    let a = Some(5).okOrElse(fail)
    let b = none.okOrElse(late)
    let av = match a {
        case Ok(n) => n
        case Err(_) => 0
    }
    let bv = match b {
        case Ok(_) => 0
        case Err(e) => if e == "late" { 7 } else { 0 }
    }
    av + bv
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_option_ok_or_else(Some(5), (lambda : _t_6661696c(host))"),
        "Some.okOrElse function callback should adapt host with zero args: {generated}"
    );
    assert!(
        generated.contains("tpz_option_ok_or_else(_t_6e6f6e65, (lambda : _t_6c617465(host))"),
        "None.okOrElse function callback should adapt host with zero args: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("Option.okOrElse Python gate failed: {e}"));
}
