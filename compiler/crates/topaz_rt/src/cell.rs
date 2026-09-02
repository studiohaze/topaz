//! §5 the rebinding cell for a captured-MUTABLE binding. A `let mut` that a
//! closure captures becomes a shared `Rc<RefCell<Value>>` — the one-binding
//! analog of the interpreter's whole-env `Rc<RefCell<Scope>>` capture, so a
//! mutation made BEFORE, AFTER, or INSIDE a closure is visible everywhere the
//! cell is shared (the interpreter's live-environment semantics).
//!
//! Every helper DROPS the `RefCell` borrow before it returns, so a borrow is
//! never held across a `.await`, across another cell access, or to the end of
//! the enclosing statement — which is the only way these could panic. The
//! emitter composes `cell_get`/`cell_set` so the argument fully evaluates
//! BEFORE the `borrow_mut` inside `cell_set`, giving the interpreter's
//! read-then-write ordering for free.

use std::cell::RefCell;
use std::rc::Rc;

use topaz_value::Value;

/// A fresh cell holding `v`.
pub fn cell_new(v: Value) -> Rc<RefCell<Value>> {
    Rc::new(RefCell::new(v))
}

/// The cell's current value, cloned — the `Ref` is dropped before returning,
/// so the result owns no borrow.
pub fn cell_get(cell: &Rc<RefCell<Value>>) -> Value {
    cell.borrow().clone()
}

/// Overwrite the cell — the `RefMut` is dropped before returning. The new
/// value is evaluated by the caller (the argument) BEFORE this borrows, so no
/// borrow is live while the right-hand side runs.
pub fn cell_set(cell: &Rc<RefCell<Value>>, v: Value) {
    *cell.borrow_mut() = v;
}
