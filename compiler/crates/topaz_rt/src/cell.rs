//! Shared cells for mutable bindings captured by generated closures.
//! Each helper releases its RefCell borrow before returning. Callers evaluate
//! a replacement value before cell_set acquires the mutable borrow.

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
