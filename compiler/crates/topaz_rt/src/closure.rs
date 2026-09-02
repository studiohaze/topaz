//! The emitted-code callable ABI (CDR-006 §4). Generated code never
//! writes a `TpzCall` impl by hand: a lambda becomes an
//! [`EmittedClosure`] wrapping a Rust closure that returns the body's
//! future plus the param names (for arity faults), and a call goes
//! through [`call_value`]. The interpreter drives ITS closures by
//! downcast + frames instead; both reach the same observable result
//! because the body lowering and the value model are shared.

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use topaz_value::{
    Builtin, CALL_DEPTH_LIMIT, CallFuture, CallbackHofKind, CallbackHofStep, CallbackKeyStep,
    CallbackMapHofKind, CallbackMapHofStep, CallbackMapUpdateStep, CallbackOkOrElseStep,
    CallbackReceiverMapKind, CallbackReceiverMapStep, CallbackRetainStep, OrderedMap,
    ReceiverBuiltinRoute, RtCx, RtError, Span, TpzCall, Value, bind_builtin_named_args,
    bind_named_arg_slots, call_host_builtin, call_method, call_pure_builtin, call_resource_method,
    codes, exact_args, fault, index_value, iterable_items, member_value, no_member_fault,
    prepare_callback_hof, prepare_callback_key_collection, prepare_callback_map_hof,
    prepare_callback_map_update, prepare_callback_ok_or_else, prepare_callback_receiver_map_kind,
    prepare_callback_retain, project_lispex_application_host_value, receiver_builtin_by_kind,
    recursion_fault, sorted_by_keys,
};

/// §4 enter one call level (restored on EVERY exit path by the RAII guard) and run the
/// closure body. The recursion CHECK happens at each caller site BEFORE arity binding
/// (so the cap fault precedes an arity fault, matching the interpreter's `apply_call`
/// order); this only performs the matching `enter_call`. All four closure-invoke paths
/// (`call_value`, `call_value_spread`, `call_value_named`, `call_value_spread_named`) route
/// through here.
fn call_closure_guarded(c: Rc<dyn TpzCall>, cx: RtCx, args: Vec<Value>) -> CallFuture {
    Box::pin(async move {
        let _guard = cx.enter_call();
        c.call(cx, args).await
    })
}

/// §4 the SHARED recursion-cap check — `Some(fault-future)` when entering one more call
/// would exceed [`CALL_DEPTH_LIMIT`] (the interpreter's `apply_call` makes the same check
/// BEFORE arity/binding, so the cap fault wins over an arity fault at the boundary).
/// Each closure-invoke path calls this FIRST, before any arity work.
fn recursion_cap_exceeded(cx: &RtCx, span: Span) -> Option<CallFuture> {
    if cx.call_depth() >= CALL_DEPTH_LIMIT {
        Some(Box::pin(async move { Err(recursion_fault(span)) }))
    } else {
        None
    }
}

enum ClosureCallTail {
    None,
    Spread(Rc<RefCell<Vec<Value>>>),
}

/// Bind and invoke an emitted closure for every generated call shape. Positional
/// overflow, named-slot ownership, lazy defaults, and final invocation have one
/// authority; a spread tail additionally requires every positionally skipped fixed
/// slot to have a default before named binding and is appended after surplus.
fn call_closure(
    c: Rc<dyn TpzCall>,
    mut positional: Vec<Value>,
    named: Vec<(String, Value)>,
    tail: ClosureCallTail,
    cx: RtCx,
    span: Span,
) -> CallFuture {
    if let Some(fault) = recursion_cap_exceeded(&cx, span) {
        return fault;
    }
    let arity = c.arity();
    if matches!(&tail, ClosureCallTail::Spread(_)) && !c.is_variadic() {
        let message = if positional.len() > arity {
            format!("expected {arity} argument(s), found more")
        } else {
            "spread arguments require a variadic parameter (§5)".to_string()
        };
        return Box::pin(async move { Err(fault(codes::GUARD_ARITY, message, span)) });
    }
    let surplus = if c.is_variadic() && positional.len() > arity {
        positional.split_off(arity)
    } else {
        Vec::new()
    };
    if positional.len() > arity {
        return Box::pin(async move {
            Err(fault(
                codes::GUARD_ARITY,
                format!("expected {arity} argument(s), found more"),
                span,
            ))
        });
    }
    let positional_filled = positional.len();
    if matches!(&tail, ClosureCallTail::Spread(_))
        && (positional_filled..arity).any(|index| !c.has_param_default(index))
    {
        return Box::pin(async move {
            Err(fault(
                codes::GUARD_ARITY,
                "a spread argument cannot skip an unsatisfied fixed parameter (§5)",
                span,
            ))
        });
    }
    let slots = match bind_named_arg_slots(
        positional.into_iter().map(Some).collect(),
        arity,
        |index| c.param_name(index),
        named,
        span,
    ) {
        Ok(slots) => slots,
        Err(error) => return Box::pin(async move { Err(error) }),
    };
    Box::pin(async move {
        let mut args = Vec::with_capacity(arity + surplus.len());
        for (index, slot) in slots.into_iter().enumerate() {
            match slot {
                Some(value) => args.push(value),
                None => match c.param_default(index, cx.clone()) {
                    Some(default) => args.push(default.await?),
                    None => {
                        let name = c.param_name(index).unwrap_or_default();
                        return Err(fault(
                            codes::GUARD_ARITY,
                            format!("missing argument for parameter `{name}` (§5)"),
                            span,
                        ));
                    }
                },
            }
        }
        args.extend(surplus);
        if let ClosureCallTail::Spread(items) = tail {
            args.extend(items.borrow().iter().cloned());
        }
        call_closure_guarded(c, cx, args).await
    })
}

/// §4 the SHARED recursion guard for a v5.4 NATIVE (monomorphized) `async fn` body.
///
/// A native scalar function is emitted as `async fn f(cx, __call_span, args…)` whose
/// FIRST statement is `let _guard = __native_enter_call(&cx, __call_span)?;`. This does
/// EXACTLY what the boxed `call_value` path does at a call boundary — check the cap at the
/// CALL-EXPRESSION span (returning [`recursion_fault`] when entering would exceed
/// [`CALL_DEPTH_LIMIT`]), then [`RtCx::enter_call`] to bump the depth and hand back the
/// RAII [`topaz_value::CallDepthGuard`] that restores it on every exit path. The native callsite passes
/// the call-expression span and evaluates arguments BEFORE the call, so the order
/// (args → cap-check-at-call-span → enter → body) is byte-identical to interp+boxed, and
/// the `GUARD_RECURSION` code/message/span cannot drift (it routes through the SAME
/// `recursion_fault`/`enter_call` leaf both other engines use).
pub fn __native_enter_call(cx: &RtCx, span: Span) -> Result<topaz_value::CallDepthGuard, RtError> {
    if cx.call_depth() >= CALL_DEPTH_LIMIT {
        return Err(recursion_fault(span));
    }
    Ok(cx.enter_call())
}

// --- v5.4 NATIVE Array<scalar> read boundary (CDR-006 §2 shared-leaf) ---
//
// A native island may hold a BOXED `Value::Array` of a CONCRETE scalar element
// type (the array stays boxed; only the READS are native). `arr[i]` routes the
// index through the SHARED `index_value` leaf — so the out-of-bounds fault
// (`FAULT_INDEX`, exact message + SPAN) is BYTE-IDENTICAL to the interpreter —
// then unboxes the element to a bare scalar. The element type is the array's
// declared scalar element (the program type-checked clean → the runtime element
// IS that scalar, so the mismatch arm is unreachable on the native path; it
// still FAULTS rather than panics, defensively, so it can never be UB).

/// The "internal" fault for the unreachable element-type mismatch (a checked
/// `Array<int>` whose runtime element is not an `int`): the checker proves this
/// cannot happen on the native path, so this is a defended-unreachable fault, not
/// a divergence — native runs only on type-checked units (`--unchecked` is always
/// boxed), where the element type is guaranteed.
fn native_elem_mismatch(want: &str, got: &Value, span: Span) -> RtError {
    fault(
        codes::GUARD_TYPE,
        format!(
            "native array element expected `{want}`, found `{}` (internal: unreachable on a checked build)",
            got.kind()
        ),
        span,
    )
}

/// `arr[i] -> i64` for a native `Array<int>` read. The OOB fault is the SHARED
/// `index_value` fault (byte-identical to interp/boxed).
pub fn native_index_int(arr: &Value, i: i64, span: Span) -> Result<i64, RtError> {
    match index_value(arr.clone(), Value::Int(i), span)? {
        Value::Int(n) => Ok(n),
        other => Err(native_elem_mismatch("int", &other, span)),
    }
}

/// `arr[i] -> f64` for a native `Array<float>` read.
pub fn native_index_float(arr: &Value, i: i64, span: Span) -> Result<f64, RtError> {
    match index_value(arr.clone(), Value::Int(i), span)? {
        Value::Float(x) => Ok(x),
        other => Err(native_elem_mismatch("float", &other, span)),
    }
}

/// `arr[i] -> bool` for a native `Array<bool>` read.
pub fn native_index_bool(arr: &Value, i: i64, span: Span) -> Result<bool, RtError> {
    match index_value(arr.clone(), Value::Int(i), span)? {
        Value::Bool(b) => Ok(b),
        other => Err(native_elem_mismatch("bool", &other, span)),
    }
}

/// `arr[i] -> String` for a native `Array<string>` read.
pub fn native_index_string(arr: &Value, i: i64, span: Span) -> Result<String, RtError> {
    match index_value(arr.clone(), Value::Int(i), span)? {
        Value::Str(s) => Ok(s.to_string()),
        other => Err(native_elem_mismatch("string", &other, span)),
    }
}

/// Unbox a `Value::Int` item for a native scalar `for` loop.
pub fn native_unbox_int(value: Value, span: Span) -> Result<i64, RtError> {
    match value {
        Value::Int(n) => Ok(n),
        other => Err(native_elem_mismatch("int", &other, span)),
    }
}

/// Unbox a `Value::Float` item for a native scalar `for` loop.
pub fn native_unbox_float(value: Value, span: Span) -> Result<f64, RtError> {
    match value {
        Value::Float(x) => Ok(x),
        other => Err(native_elem_mismatch("float", &other, span)),
    }
}

/// Unbox a `Value::Bool` item for a native scalar `for` loop.
pub fn native_unbox_bool(value: Value, span: Span) -> Result<bool, RtError> {
    match value {
        Value::Bool(b) => Ok(b),
        other => Err(native_elem_mismatch("bool", &other, span)),
    }
}

/// Unbox a `Value::Str` item for a native scalar `for` loop.
pub fn native_unbox_string(value: Value, span: Span) -> Result<String, RtError> {
    match value {
        Value::Str(s) => Ok(s.to_string()),
        other => Err(native_elem_mismatch("string", &other, span)),
    }
}

/// `arr.length -> i64` for a native array read — routes through the SHARED
/// `member_value` leaf (the `Array.length` arm always yields `Value::Int(len)`),
/// so the length value matches the interpreter exactly.
pub fn native_array_len(arr: &Value, span: Span) -> Result<i64, RtError> {
    match member_value(arr, "length", span)? {
        Some(Value::Int(n)) => Ok(n),
        // `member_value` returns `Some(Value::Int(..))` for every `Array.length`;
        // anything else means the boundary local was not an Array — unreachable on
        // the native path (the local is a typed `Array<scalar>`), defended here.
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "native `.length` expected an array, found `{}` (internal: unreachable on a checked build)",
                other.map(|v| v.kind()).unwrap_or("()")
            ),
            span,
        )),
    }
}

/// §7 invoke a closure WITHOUT consuming a recursion level — for the `concurrent` arm
/// wrapper ONLY. Each arm is a synthetic zero-arg closure (an implementation artifact,
/// not a user call); counting it would make an arm start one level deep, diverging from
/// the interpreter (which runs an arm body as a raw eval at the ambient depth). The arm's
/// OWN user calls are counted within its per-arm depth scope ([`crate::depth_scoped`]).
pub fn call_value_uncounted(callee: Value, args: Vec<Value>, cx: RtCx, span: Span) -> CallFuture {
    match callee {
        // The arm wrapper is always a zero-arg closure → run its body directly (no cap
        // check, no `enter_call`), so the arm body executes at the ambient depth.
        Value::Closure(c) => c.call(cx, args),
        // Defensive: a non-closure callee (never produced for an arm) falls back to the
        // ordinary counted path so any other shape keeps exact `call_value` semantics.
        other => call_value(other, args, cx, span),
    }
}

/// Complete one generated-runtime composed call. Every argument shape is
/// forwarded only to the left callable; the right callable always receives the
/// single intermediate value at the original call span.
fn call_composed(
    pair: Rc<(Value, Value)>,
    cx: RtCx,
    span: Span,
    call_left: impl FnOnce(Value, RtCx, Span) -> CallFuture + 'static,
) -> CallFuture {
    let left = pair.0.clone();
    let right = pair.1.clone();
    Box::pin(async move {
        let mid = call_left(left, cx.clone(), span).await?;
        call_value(right, vec![mid], cx, span).await
    })
}

/// §7 emitted parameter default. Literal defaults stay as inert `Value`s; any
/// const-shaped default that reads the defining environment is a call-time thunk.
#[derive(Clone)]
pub enum EmittedDefault {
    Value(Value),
    Thunk(Rc<dyn Fn(RtCx) -> CallFuture>),
}

impl fmt::Debug for EmittedDefault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(v) => f.debug_tuple("Value").field(v).finish(),
            Self::Thunk(_) => f.write_str("Thunk(<default>)"),
        }
    }
}

/// Wraps the Rust closure an emitted lambda lowers to, giving it the
/// `TpzCall` ABI. `F` is the per-lambda closure type `Fn(RtCx,
/// Vec<Value>) -> CallFuture`; a NON-capturing Topaz lambda lowers to a
/// non-capturing (hence `'static`) Rust closure, so `EmittedClosure<F>`
/// is `'static` and object-safe behind `Rc<dyn TpzCall>`. `params` are
/// the parameter names, used only to raise the §5 arity faults the
/// interpreter would (since `call` carries no span).
pub struct EmittedClosure<F> {
    pub call: F,
    pub params: &'static [&'static str],
    /// §7 per-parameter DEFAULT values (parallel to `params`): `Some` for a
    /// parameter with a default, `None` otherwise. Empty for a closure with no
    /// defaults (every lambda, and a `function` without default parameters), so
    /// `has_param_default` returns `false` for all positions.
    pub defaults: Vec<Option<EmittedDefault>>,
    /// §5 whether the source `function` had a trailing `...rest` parameter. When
    /// `true`, `params`/`defaults` cover only the FIXED parameters and the
    /// emitted `call` body collects the surplus positional arguments into the
    /// variadic's array; `call_value` then accepts any arg count at or above the
    /// fixed arity. `false` for every lambda and non-variadic `function`.
    pub variadic: bool,
}

impl<F> fmt::Debug for EmittedClosure<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // §2 renders a function as `<function>`; the closure body is not
        // inspectable.
        f.write_str("<function>")
    }
}

impl<F> TpzCall for EmittedClosure<F>
where
    F: Fn(RtCx, Vec<Value>) -> CallFuture + 'static,
{
    fn as_any(&self) -> &dyn Any {
        // Only the interpreter downcasts (to recover ITS `ClosureData`);
        // an emitted closure is never the downcast target, so this just
        // satisfies the trait.
        self
    }
    fn name(&self) -> Option<&str> {
        None
    }
    fn call(&self, cx: RtCx, args: Vec<Value>) -> CallFuture {
        (self.call)(cx, args)
    }
    fn arity(&self) -> usize {
        self.params.len()
    }
    fn param_name(&self, n: usize) -> Option<&str> {
        self.params.get(n).copied()
    }
    fn has_param_default(&self, n: usize) -> bool {
        self.defaults.get(n).is_some_and(Option::is_some)
    }
    fn param_default(&self, n: usize, cx: RtCx) -> Option<CallFuture> {
        match self.defaults.get(n).and_then(Option::as_ref) {
            Some(EmittedDefault::Value(value)) => {
                let value = value.clone();
                Some(Box::pin(async move { Ok(value) }))
            }
            Some(EmittedDefault::Thunk(thunk)) => Some(thunk(cx)),
            None => None,
        }
    }
    fn is_variadic(&self) -> bool {
        self.variadic
    }
}

/// The emitted call site (CDR-006 §4): invoke `callee` with `args`. This
/// slice produces only CLOSURE values as callables, so only `Value::
/// Closure` is callable here; any other value is `GUARD_NOT_CALLABLE`,
/// the SAME code, message, and span the interpreter's `apply_call`
/// raises. Arity is checked HERE (the call site has the span; `TpzCall::
/// call` does not), with the interpreter's exact messages — too many is
/// `expected N argument(s), found more`; too few names the first missing
/// parameter. (Lambda params take no defaults, so an exact match is
/// required.)
/// §5 a direct call carrying a SPREAD argument to a CLOSURE. `positional` is the
/// fixed-slot prefix (args before the first spread) and `spread` is the variadic-tail
/// region as a single Array — the emitter ALWAYS synthesizes it (a `{ … Value::array(__sp)
/// }` block) by flattening every from-first-spread-onward arg up to the first named arg in
/// source order (positionals pushed, spreads `call_spread_extend`'d), so this receives one
/// Array regardless of how many spreads / interleaved positionals the source had.
/// Mirrors the interpreter's `apply_call` spread path EXACTLY: a spread fills only
/// the variadic tail (never a fixed slot), so a NON-variadic callee is refused, a
/// spread may not skip an unsatisfied (default-less) fixed parameter, and the spread
/// value must be an `Array`. The faults are byte-identical to `apply_call`
/// (`spread_span` for the non-Array fault, `span` — the call — for the arity ones).
/// Then the positional prefix (padded with defaults to the fixed arity) and the
/// spread elements are concatenated and the closure is called — so the spread lands
/// only in the variadic tail. (Scope: the no-named spread path; a call that ALSO carries
/// named args routes to [`call_value_spread_named`] instead. A non-Closure callee is
/// handled here too: a `Builtin` reports the variadic-required §5 fault, a `Composed`
/// forwards the spread to its left operand, and any other value is not callable.)
pub fn call_value_spread(
    callee: Value,
    positional: Vec<Value>,
    spread: Value,
    cx: RtCx,
    span: Span,
    spread_span: Span,
) -> CallFuture {
    let Value::Array(items) = spread else {
        return Box::pin(async move {
            Err(fault(
                codes::GUARD_TYPE,
                "a spread argument must be an Array (§5)",
                spread_span,
            ))
        });
    };
    let Value::Closure(c) = callee else {
        // §5 a spread to a free `Builtin` (always non-variadic) is rejected by the
        // interpreter with "spread arguments require a variadic parameter". A NON-callable
        // value (an int, a record, …) instead faults "`X` is not callable" — defer THAT
        // case to `call_value` (with the flattened args) so it raises the interpreter's
        // exact not-callable fault. (Never flatten a Builtin into `call_value`: it would
        // RUN the callee instead of rejecting the spread.)
        //
        // The ONLY variadic builtins are `Array.of`/`Set.of`, and they CANNOT reach here
        // as a callee VALUE — `Array`/`Set` are static heads, not bound values
        // (`let f = Array.of` faults TPZ5002 "`Array` is not bound" in both engines), and
        // a direct `Array.of(...xs)` is handled (and supported — flattened) by the
        // constructor arm before the generic call. So the reachable free builtins
        // (print/toInt/open/map/filter/reduce) are all non-variadic — the reject is right.
        return match callee {
            Value::Builtin { .. } => Box::pin(async move {
                Err(fault(
                    codes::GUARD_ARITY,
                    "spread arguments require a variadic parameter (§5)",
                    span,
                ))
            }),
            // §11 a composed `f >> g` forwards the spread to the LEFT operand `f` (which
            // CAN be variadic — `(...xs) -> _ >> g` accepts a spread), then applies `g` to
            // the single result, matching the interpreter's `apply_call` `Composed` arm.
            Value::Composed(pair) => call_composed(pair, cx, span, move |left, cx, span| {
                call_value_spread(left, positional, Value::Array(items), cx, span, spread_span)
            }),
            other => {
                let mut args = positional;
                args.extend(items.borrow().iter().cloned());
                call_value(other, args, cx, span)
            }
        };
    };
    call_closure(
        c,
        positional,
        Vec::new(),
        ClosureCallTail::Spread(items),
        cx,
        span,
    )
}

/// One generated spread-plus-named call payload. The callee, three argument
/// regions, and their two diagnostic spans move through composed-call recursion
/// as one source-call identity; the runtime context remains the separate
/// execution input.
pub struct SpreadNamedCall {
    callee: Value,
    positional: Vec<Value>,
    spread: Value,
    named: Vec<(String, Value)>,
    call_span: Span,
    spread_span: Span,
}

impl SpreadNamedCall {
    pub fn new(
        callee: Value,
        positional: Vec<Value>,
        spread: Value,
        named: Vec<(String, Value)>,
        call_span: Span,
        spread_span: Span,
    ) -> Self {
        Self {
            callee,
            positional,
            spread,
            named,
            call_span,
            spread_span,
        }
    }
}

/// §5 a direct call carrying BOTH a SPREAD and NAMED args (`f(pos…, ...xs, name: v)`),
/// mirroring the interpreter's `apply_call` acc+spread+named path EXACTLY for every callee
/// kind:
///
/// * **Closure** — the union of [`call_value_spread`] (the spread fills the variadic tail;
///   a non-variadic callee → "requires a variadic parameter") and [`call_value_named`]'s
///   fixed-slot fill. A VARIADIC closure's positional SURPLUS (beyond the fixed arity)
///   overflows into the variadic ahead of the spread items. A no-default fixed slot NOT
///   filled by a positional faults "cannot skip an unsatisfied fixed parameter" BEFORE
///   named binding (a later named can NOT rescue it); named then override remaining
///   (defaulted) fixed slots by name, with given-twice / no-parameter faults.
/// * **Builtin** — every builtin is non-variadic, so a spread is rejected; but WHEN there
///   are named args the interpreter does the full slot binding FIRST (positionals + named
///   by name + a MISSING check for an unfilled slot — given-twice / no-parameter / missing
///   faults), THEN rejects the spread. With NO named args the binding is skipped, so the
///   rejection is reached directly (`g(...[])` → "requires variadic", but `map(...[], xs:
///   a)` → "missing argument for `f`").
/// * **Composed** `f >> g` — forward the whole acc+spread+named call to `f`, then apply
///   `g` to the single result (the interpreter's `KComposeAfter`).
/// * anything else — not callable (deferred to [`call_value`] with the flattened args).
pub fn call_value_spread_named(call: SpreadNamedCall, cx: RtCx) -> CallFuture {
    let SpreadNamedCall {
        callee,
        positional,
        spread,
        named,
        call_span: span,
        spread_span,
    } = call;
    let Value::Array(spread_items) = spread else {
        return Box::pin(async move {
            Err(fault(
                codes::GUARD_TYPE,
                "a spread argument must be an Array (§5)",
                spread_span,
            ))
        });
    };
    match callee {
        Value::Closure(c) => call_closure(
            c,
            positional,
            named,
            ClosureCallTail::Spread(spread_items),
            cx,
            span,
        ),
        // §22 a builtin VALUE is non-variadic, so the spread is rejected — but the
        // interpreter (machine.rs `apply_call` Builtin arm) does the FULL named-arg slot
        // binding FIRST when there are named args: positionals fill slots, named fill by
        // name (given-twice incl. a positional-filled slot, no-parameter), and an unfilled
        // slot faults "missing argument" — ALL before the spread rejection. With NO named
        // args, the binding is skipped and the spread rejection is reached directly (so
        // `g(...[])` → "requires variadic", NOT "missing"). The only variadic builtins
        // (`Array.of`/`Set.of`) cannot reach here as a callee VALUE.
        Value::Builtin { kind, recv } => {
            if !named.is_empty()
                && let Err(error) =
                    bind_builtin_named_args(kind, recv.is_some(), positional, named, span)
            {
                return Box::pin(async move { Err(error) });
            }
            Box::pin(async move {
                Err(fault(
                    codes::GUARD_ARITY,
                    "spread arguments require a variadic parameter (§5)",
                    span,
                ))
            })
        }
        // §11 a composed function `f >> g` forwards the whole acc+spread+named call to the
        // LEFT operand, then applies the RIGHT to the single result — the interpreter's
        // `apply_call` `Composed` arm threads `argc/named/spread` into `f`.
        Value::Composed(pair) => call_composed(pair, cx, span, move |left, cx, span| {
            call_value_spread_named(
                SpreadNamedCall::new(
                    left,
                    positional,
                    Value::Array(spread_items),
                    named,
                    span,
                    spread_span,
                ),
                cx,
            )
        }),
        // A NON-callable value: the not-callable check precedes any binding, so the named
        // args are irrelevant — defer to `call_value` with the flattened positional+spread
        // so it raises the interpreter's exact "`X` is not callable" fault.
        other => {
            let mut args = positional;
            args.extend(spread_items.borrow().iter().cloned());
            call_value(other, args, cx, span)
        }
    }
}

/// The `(min, max)` argument-arity range of a callable VALUE (`max = None` for
/// variadic), or `None` if not callable — the emitted mirror of the interpreter's
/// `callable_arity` (machine.rs). An emitted `Value::Closure` exposes the FIXED
/// arity via `TpzCall::arity()` (the variadic tail is EXCLUDED, per `call_value`)
/// and per-fixed-parameter defaults via `param_default`, so a fixed slot with a
/// default is not "required"; a `Builtin` uses the SHARED `Builtin::arity_range`
/// (no drift with the interpreter); a `Composed` is its LEFT operand's shape.
fn callable_arity(value: &Value) -> Option<(usize, Option<usize>)> {
    match value {
        Value::Closure(c) => {
            let fixed = c.arity();
            let required = (0..fixed).filter(|&i| !c.has_param_default(i)).count();
            Some((required, if c.is_variadic() { None } else { Some(fixed) }))
        }
        Value::Builtin { kind, .. } => Some(kind.arity_range()),
        Value::Composed(pair) => callable_arity(&pair.0),
        _ => None,
    }
}

/// Whether `value` conforms to a FUNCTION type of `n_fixed` fixed parameters and
/// `type_variadic` — the emitted mirror of the interpreter's `type_matches`
/// `TypeKind::Function` arm. Shape-only: parameter and return types are NOT
/// runtime-inspectable; only the callable's arity range is. A variadic function
/// type accepts only a variadic-capable callable.
pub fn callable_shape_matches(value: &Value, n_fixed: usize, type_variadic: bool) -> bool {
    match callable_arity(value) {
        None => false,
        Some((min, max)) => {
            if type_variadic {
                max.is_none() && min <= n_fixed
            } else {
                min <= n_fixed && max.is_none_or(|m| n_fixed <= m)
            }
        }
    }
}

/// Drive one shared callback-HOF execution while generated code owns callback
/// invocation through [`call_value`]. Direct and first-class calls use this same
/// driver.
pub fn call_callback_hof(
    kind: CallbackHofKind,
    args: Vec<Value>,
    cx: RtCx,
    span: Span,
) -> CallFuture {
    Box::pin(async move {
        let mut step = prepare_callback_hof(kind, args, span)?.next();
        loop {
            match step {
                CallbackHofStep::Complete(value) => return Ok(value),
                CallbackHofStep::Call {
                    pending,
                    callee,
                    args,
                } => {
                    let result = call_value(callee, args, cx.clone(), span).await?;
                    step = pending.resume(result, span)?.next();
                }
            }
        }
    })
}

/// Collect `f(item)` keys in item order through the same evaluator-independent
/// state consumed by the interpreter. Generated `sortedBy` and `sortBy` share
/// this callback driver and choose their return-new or write-back result after
/// the final stable sort.
pub async fn collect_callback_keys(
    items: Vec<Value>,
    callback: Value,
    cx: RtCx,
    span: Span,
) -> Result<(Vec<Value>, Vec<Value>), RtError> {
    let mut step = prepare_callback_key_collection(items, callback).next();
    loop {
        match step {
            CallbackKeyStep::Complete { items, keys } => return Ok((items, keys)),
            CallbackKeyStep::Call(pending) => {
                let (callee, item) = pending.invocation();
                let key = call_value(callee, vec![item], cx.clone(), span).await?;
                step = pending.resume(key).next();
            }
        }
    }
}

/// Drive array `retain` through the shared evaluator-independent predicate
/// state and return the kept snapshot for the caller's final write-back.
pub async fn collect_retained_items(
    items: Vec<Value>,
    callback: Value,
    cx: RtCx,
    span: Span,
) -> Result<Vec<Value>, RtError> {
    let mut step = prepare_callback_retain(items, callback).next();
    loop {
        match step {
            CallbackRetainStep::Complete(kept) => return Ok(kept),
            CallbackRetainStep::Call(pending) => {
                let (callee, item) = pending.invocation();
                let predicate = call_value(callee, vec![item], cx.clone(), span).await?;
                step = pending.resume(predicate, span)?.next();
            }
        }
    }
}

/// Drive `Map.filter` or `mapValues` through the shared pair-order and
/// ordered-result state machine.
pub async fn call_callback_map_hof(
    kind: CallbackMapHofKind,
    pairs: Vec<(Value, Value)>,
    callback: Value,
    cx: RtCx,
    span: Span,
) -> Result<Value, RtError> {
    let mut step = prepare_callback_map_hof(kind, pairs, callback).next();
    loop {
        match step {
            CallbackMapHofStep::Complete(value) => return Ok(value),
            CallbackMapHofStep::Call {
                pending,
                callee,
                args,
            } => {
                let result = call_value(callee, args, cx.clone(), span).await?;
                step = pending.resume(result, span)?.next();
            }
        }
    }
}

/// Drive `Map.update` through the shared probe/commit transition. The absent
/// path inserts `initial` without invoking the callback; the present path keeps
/// the existing insertion slot when committing the callback result.
pub async fn call_callback_map_update(
    map: Rc<std::cell::RefCell<OrderedMap>>,
    key: Value,
    initial: Value,
    callback: Value,
    cx: RtCx,
    span: Span,
) -> Result<Value, RtError> {
    match prepare_callback_map_update(map, key, initial, callback, span)? {
        CallbackMapUpdateStep::Complete(value) => Ok(value),
        CallbackMapUpdateStep::Call {
            pending,
            callee,
            existing,
        } => {
            let result = call_value(callee, vec![existing], cx, span).await?;
            pending.resume(result, span)
        }
    }
}

/// Drive receiver `map` through the shared Option/Result transition and
/// delegate every other receiver to the iterable callback-HOF authority.
pub async fn call_callback_receiver_map(
    receiver: Value,
    callback: Value,
    cx: RtCx,
    span: Span,
) -> Result<Value, RtError> {
    call_callback_receiver_map_kind(
        CallbackReceiverMapKind::Map,
        receiver,
        callback,
        cx,
        span,
        span,
    )
    .await
}

/// Drive `Option.okOrElse` through the shared lazy Option-to-Result transition.
pub async fn call_callback_ok_or_else(
    receiver: Value,
    callback: Value,
    cx: RtCx,
    member_span: Span,
    call_span: Span,
) -> Result<Value, RtError> {
    match prepare_callback_ok_or_else(receiver, callback) {
        CallbackOkOrElseStep::Complete(value) => Ok(value),
        CallbackOkOrElseStep::Call { pending, callee } => {
            let error = call_value(callee, Vec::new(), cx, call_span).await?;
            Ok(pending.resume(error))
        }
        CallbackOkOrElseStep::Unsupported { receiver } => {
            Err(no_member_fault(&receiver, "okOrElse", member_span))
        }
    }
}

/// Drive `Option.flatMap` and `Result.flatMap` through the receiver-map
/// transition with identity callback completion.
pub async fn call_callback_receiver_flat_map(
    receiver: Value,
    callback: Value,
    cx: RtCx,
    member_span: Span,
    call_span: Span,
) -> Result<Value, RtError> {
    call_callback_receiver_map_kind(
        CallbackReceiverMapKind::FlatMap,
        receiver,
        callback,
        cx,
        member_span,
        call_span,
    )
    .await
}

async fn call_callback_receiver_map_kind(
    kind: CallbackReceiverMapKind,
    receiver: Value,
    callback: Value,
    cx: RtCx,
    member_span: Span,
    call_span: Span,
) -> Result<Value, RtError> {
    match prepare_callback_receiver_map_kind(kind, receiver, callback) {
        CallbackReceiverMapStep::Complete(value) => Ok(value),
        CallbackReceiverMapStep::Call {
            pending,
            callee,
            input,
        } => {
            let result = call_value(callee, vec![input], cx, call_span).await?;
            Ok(pending.resume(result))
        }
        CallbackReceiverMapStep::Delegate { receiver, callback } => {
            call_callback_hof(
                CallbackHofKind::Map,
                vec![receiver, callback],
                cx,
                call_span,
            )
            .await
        }
        CallbackReceiverMapStep::Unsupported { receiver } => {
            Err(no_member_fault(&receiver, "flatMap", member_span))
        }
    }
}

/// Drive a receiver-bound callback builtin after the shared receiver catalog
/// has proved the `(receiver, Builtin)` identity. Direct member calls and
/// first-class receiver values therefore consume the same callback state and
/// write-back boundaries.
fn call_bound_callback_builtin(
    kind: Builtin,
    receiver: Value,
    member: &'static str,
    args: Vec<Value>,
    cx: RtCx,
    span: Span,
) -> CallFuture {
    Box::pin(async move {
        if let Some(hof) = CallbackHofKind::from_builtin(kind) {
            let mut bound_args = Vec::with_capacity(args.len() + 1);
            bound_args.push(receiver);
            bound_args.extend(args);
            return call_callback_hof(hof, bound_args, cx, span).await;
        }
        match kind {
            Builtin::MapMapValues | Builtin::MapFilter => {
                let [callback] = exact_args(args, span)?;
                let Value::Map(map) = &receiver else {
                    return Err(no_member_fault(&receiver, member, span));
                };
                let callback_kind = if kind == Builtin::MapFilter {
                    CallbackMapHofKind::Filter
                } else {
                    CallbackMapHofKind::MapValues
                };
                let pairs = map.borrow().pairs();
                call_callback_map_hof(callback_kind, pairs, callback, cx, span).await
            }
            Builtin::MapUpdate => {
                let [key, initial, callback] = exact_args(args, span)?;
                let Value::Map(map) = receiver else {
                    return Err(no_member_fault(&receiver, member, span));
                };
                call_callback_map_update(map, key, initial, callback, cx, span).await
            }
            Builtin::ArrSortBy | Builtin::ArrRetain => {
                let [callback] = exact_args(args, span)?;
                let Value::Array(array) = &receiver else {
                    return Err(no_member_fault(&receiver, member, span));
                };
                let items = array.borrow().clone();
                let output = if kind == Builtin::ArrSortBy {
                    let (items, keys) = collect_callback_keys(items, callback, cx, span).await?;
                    sorted_by_keys(&items, &keys, span)?
                } else {
                    collect_retained_items(items, callback, cx, span).await?
                };
                *array.borrow_mut() = output;
                Ok(Value::Unit)
            }
            Builtin::ArrSortedBy => {
                let [callback] = exact_args(args, span)?;
                let items = iterable_items(receiver, span)?;
                let (items, keys) = collect_callback_keys(items, callback, cx, span).await?;
                Ok(Value::array(sorted_by_keys(&items, &keys, span)?))
            }
            Builtin::OkOrElse => {
                let [callback] = exact_args(args, span)?;
                call_callback_ok_or_else(receiver, callback, cx, span, span).await
            }
            Builtin::OptionMap | Builtin::ResultMap => {
                let [callback] = exact_args(args, span)?;
                call_callback_receiver_map(receiver, callback, cx, span).await
            }
            Builtin::OptionFlatMap | Builtin::ResultFlatMap => {
                let [callback] = exact_args(args, span)?;
                call_callback_receiver_flat_map(receiver, callback, cx, span, span).await
            }
            _ => Err(fault(
                codes::GUARD_NOT_CALLABLE,
                "`function` is not callable".to_string(),
                span,
            )),
        }
    })
}

pub fn call_value(callee: Value, args: Vec<Value>, cx: RtCx, span: Span) -> CallFuture {
    let mut args = args;
    if let Value::Builtin { kind, recv: None } = &callee {
        if let Some(outcome) = call_pure_builtin(*kind, &mut args, span) {
            return Box::pin(async move { outcome });
        }
        let host = cx.host();
        if let Some(outcome) = call_host_builtin(&*host, *kind, &mut args, span) {
            let project =
                cx.module_stable_nominals() && kind.lispex_application_operation().is_some();
            return Box::pin(async move {
                outcome.map(|value| {
                    if project {
                        project_lispex_application_host_value(value)
                    } else {
                        value
                    }
                })
            });
        }
        if let Some(kind) = CallbackHofKind::from_builtin(*kind) {
            return call_callback_hof(kind, args, cx, span);
        }
    }
    match callee {
        Value::Closure(c) => call_closure(c, args, Vec::new(), ClosureCallTail::None, cx, span),
        // §22.2 every first-class receiver builtin re-enters the execution route
        // recorded by the same catalog that produced its bound value. Mutable-root
        // admission happened at member acquisition; callback and resource values
        // retain their ordinary continuation and host boundaries here.
        Value::Builtin {
            kind,
            recv: Some(recv),
        } => match receiver_builtin_by_kind(&recv, kind) {
            Some(receiver) if receiver.route == ReceiverBuiltinRoute::Method => Box::pin(
                async move { call_method((*recv).clone(), receiver.name, args, span, span) },
            ),
            Some(receiver) if receiver.route == ReceiverBuiltinRoute::Resource => {
                Box::pin(async move {
                    let host = cx.host();
                    call_resource_method(&*host, (*recv).clone(), receiver.name, args, span, span)
                })
            }
            Some(receiver) => {
                call_bound_callback_builtin(kind, (*recv).clone(), receiver.name, args, cx, span)
            }
            None => Box::pin(async move {
                Err(fault(
                    codes::GUARD_NOT_CALLABLE,
                    "`function` is not callable".to_string(),
                    span,
                ))
            }),
        },
        // §11 a composed function `f >> g`: `(f >> g)(args) == g(f(args))`.
        // Apply `f` to the args, then `g` to the result — the SAME recursion the
        // interpreter performs (`apply_call` on `f`, then `KComposeAfter` applies
        // `g` to the single result), so each per-function arity / not-callable
        // fault lands at THIS call span exactly as the interpreter's does. The
        // composed value's own arity is `f`'s (checked inside the inner call).
        Value::Composed(pair) => call_composed(pair, cx, span, move |left, cx, span| {
            call_value(left, args, cx, span)
        }),
        other => {
            let kind = other.kind();
            Box::pin(async move {
                Err(fault(
                    codes::GUARD_NOT_CALLABLE,
                    format!("`{kind}` is not callable"),
                    span,
                ))
            })
        }
    }
}

/// §5/§7 the emitted call site for a call carrying NAMED arguments: the
/// positional arguments fill the leading slots, each named argument fills its
/// parameter by NAME, and any still-unfilled slot takes its default — exactly
/// the interpreter's `apply_call` closure path (positional → named → defaults).
/// The faults match the interpreter's at the call span: too many positionals
/// (`expected N argument(s), found more`), a parameter given twice
/// (`parameter <name> is given twice (§5)`), an unknown name (`no parameter
/// named <name> (§5)`), and a missing required slot (`missing argument for
/// parameter <name> (§5)`). `Closure` and first-class `Builtin` values bind
/// named arguments here, a `Composed` value forwards them to its left callable,
/// and any other value is not callable.
pub fn call_value_named(
    callee: Value,
    positional: Vec<Value>,
    named: Vec<(String, Value)>,
    cx: RtCx,
    span: Span,
) -> CallFuture {
    match callee {
        Value::Closure(c) => call_closure(c, positional, named, ClosureCallTail::None, cx, span),
        // §11 a composed function forwards the named (and positional) arguments
        // to the LEFT function, then applies the right to the result — the
        // interpreter's `apply_call` `Composed` arm threads `named` into `f`.
        Value::Composed(pair) => call_composed(pair, cx, span, move |left, cx, span| {
            call_value_named(left, positional, named, cx, span)
        }),
        // §5/§22 a builtin value fills its parameter slots by NAME via the
        // builtin's signature, then dispatches positionally through `call_value`
        // — the interpreter's `apply_call` `Builtin` named path (slots from the
        // signature, the same given-twice / no-parameter-named / missing faults).
        Value::Builtin { kind, recv } => {
            let args = match bind_builtin_named_args(kind, recv.is_some(), positional, named, span)
            {
                Ok(args) => args,
                Err(error) => return Box::pin(async move { Err(error) }),
            };
            call_value(Value::Builtin { kind, recv }, args, cx, span)
        }
        other => {
            let kind = other.kind();
            Box::pin(async move {
                Err(fault(
                    codes::GUARD_NOT_CALLABLE,
                    format!("`{kind}` is not callable"),
                    span,
                ))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_on;
    use topaz_value::{
        FileId, Host, LispexApplicationRequest, LispexApplicationResponse, ResourceId,
    };

    struct RuntimeTestHost;

    impl Host for RuntimeTestHost {
        fn print(&self, _line: &str) {}

        fn open(&self, _path: &str) -> Result<ResourceId, String> {
            Err("no host resources".to_string())
        }

        fn read(&self, _handle: ResourceId) -> Result<String, String> {
            Err("no host resources".to_string())
        }

        fn write(&self, _handle: ResourceId, _text: &str) -> Result<(), String> {
            Err("no host resources".to_string())
        }

        fn close(&self, _handle: ResourceId) {}

        fn now_millis(&self) -> u64 {
            0
        }

        fn defer_error(&self, _rendered: &str) {}

        fn input(&self) -> String {
            "runtime input".to_string()
        }

        fn lispex_application(
            &self,
            _request: LispexApplicationRequest,
        ) -> LispexApplicationResponse {
            LispexApplicationResponse::OperationalFault {
                code: "target-unavailable".into(),
                detail: None,
            }
        }
    }

    fn context() -> RtCx {
        RtCx::new(Rc::new(RuntimeTestHost))
    }

    fn test_span() -> Span {
        Span::new(FileId(0), 0, 1)
    }

    fn builtin(kind: Builtin) -> Value {
        Value::Builtin { kind, recv: None }
    }

    #[test]
    fn first_class_builtins_use_canonical_exact_arity() {
        let converted = block_on(call_value(
            builtin(Builtin::ToInt),
            vec![Value::str("42")],
            context(),
            test_span(),
        ))
        .expect("toInt value call");
        assert!(
            matches!(converted, Value::Some(value) if matches!(value.as_ref(), Value::Int(42)))
        );

        let error = block_on(call_value(
            builtin(Builtin::ToInt),
            vec![Value::str("1"), Value::str("2")],
            context(),
            test_span(),
        ))
        .expect_err("toInt extra argument");
        assert_eq!(error.code, codes::GUARD_ARITY);
        assert_eq!(error.message, "expected 1 argument(s), found 2");

        let input = block_on(call_value(
            builtin(Builtin::Input),
            Vec::new(),
            context(),
            test_span(),
        ))
        .expect("input value call");
        assert!(matches!(input, Value::Str(value) if value.as_ref() == "runtime input"));
    }

    #[test]
    fn first_class_builtins_use_canonical_optional_and_hof_arity() {
        let buffer = block_on(call_value(
            builtin(Builtin::ByteBufferAllocate),
            vec![Value::Int(3)],
            context(),
            test_span(),
        ))
        .expect("ByteBuffer.allocate default value");
        assert!(
            matches!(buffer, Value::ByteBuffer(bytes) if bytes.borrow().as_slice() == [0, 0, 0])
        );

        let error = block_on(call_value(
            builtin(Builtin::ByteBufferAllocate),
            Vec::new(),
            context(),
            test_span(),
        ))
        .expect_err("ByteBuffer.allocate missing length");
        assert_eq!(error.code, codes::GUARD_ARITY);
        assert_eq!(error.message, "expected 1..2 argument(s), found 0");

        let mapped = block_on(call_value(
            builtin(Builtin::MapFn),
            vec![
                Value::array(vec![Value::str("1"), Value::str("2")]),
                builtin(Builtin::ToInt),
            ],
            context(),
            test_span(),
        ))
        .expect("first-class map value call");
        let Value::Array(values) = mapped else {
            panic!("map value call did not return an array");
        };
        assert!(
            matches!(&values.borrow()[0], Value::Some(value) if matches!(value.as_ref(), Value::Int(1)))
        );
        assert!(
            matches!(&values.borrow()[1], Value::Some(value) if matches!(value.as_ref(), Value::Int(2)))
        );
    }
}
