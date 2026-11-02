//! Per-scope deferred actions for generated programs.
//! Actions run in reverse registration order on normal completion and control
//! returns. Faults and Err values are reported to Host::defer_error without
//! replacing the result already being returned. Ordinary faults do not drain defers.

use std::cell::RefCell;
use std::rc::Rc;

use topaz_value::{RtCx, Value, render};

/// A function scope's deferred-action stack. SHARED (`Rc<RefCell<…>>`) between the
/// outer closure body (which drains it after the body's inner async block) and that
/// inner block (which pushes a zero-arg closure as each `defer` statement is
/// reached), so an early exit that never reached a `defer` leaves it unregistered —
/// exactly the interpreter's per-scope `defers` Vec.
pub type DeferStack = Rc<RefCell<Vec<Value>>>;

/// A fresh empty defer stack for one function scope.
pub fn defer_stack() -> DeferStack {
    Rc::new(RefCell::new(Vec::new()))
}

/// Push a deferred-action closure onto the scope's stack (the lowering of a `defer`
/// statement). Registration is positional: a `defer` not yet reached is not pushed.
pub fn defer_push(defers: &DeferStack, action: Value) {
    defers.borrow_mut().push(action);
}

/// Drain the scope's defers LIFO, invoking each zero-arg action closure against
/// `cx`. Routing is byte-identical to the interpreter's `drain_defers`: a fault →
/// `host.defer_error("{code}: {message}")`; a returned `Value::Err(inner)` →
/// `host.defer_error(render(inner))`; any other outcome is ignored. Errors are
/// CONTAINED — draining always runs every registered action and never changes the
/// in-flight result.
pub async fn run_defers(defers: &DeferStack, cx: &RtCx) {
    loop {
        let next = defers.borrow_mut().pop();
        let Some(action) = next else { break };
        let result = match action {
            Value::Closure(c) => c.call(cx.clone(), Vec::new()).await,
            // The emitter only ever pushes a zero-arg closure; anything else is an
            // internal invariant slip — ignore rather than panic (never reachable).
            _ => continue,
        };
        match result {
            Err(e) => cx.host().defer_error(&format!("{}: {}", e.code, e.message)),
            Ok(Value::Err(inner)) => cx.host().defer_error(&render(&inner)),
            Ok(_) => {}
        }
    }
}
