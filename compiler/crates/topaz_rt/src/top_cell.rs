//! §7 top-level forward-reference support for emitted code. A top-level
//! `function` is bound POSITIONALLY (it is not hoisted: SPEC.md §7, the checker's
//! forward pass, and the interpreter all bind it when its declaration executes),
//! but a function BODY runs at CALL time and may name a top-level function declared
//! LATER in the module. To lower that, each top-level (non-builtin-named) function
//! binds a `TopCell` seeded UNINITIALIZED, `top_cell_set` at the declaration's
//! position; a reference reads through `top_cell_get`, which faults `GUARD_UNBOUND`
//! (the interpreter's `ExprKind::Ident` lookup miss, machine.rs) when the cell is
//! still unfilled — so a forward call that races ahead of the callee's declaration
//! faults `TPZ5002 "\`name\` is not bound"` byte-identically in BOTH engines, while
//! a call after the declaration resolves. (Distinct from the block-local recursion
//! `ImmCell`, which seeds `Value::Unit`; a top cell must distinguish "declared but
//! not yet run" from a legitimate `Unit` value, hence `Option`.)

use std::cell::RefCell;
use std::rc::Rc;

use topaz_value::{FileId, RtError, Span, Value, codes, fault};

/// A top-level forward-reference cell: `None` until its declaration executes.
pub type TopCell = Rc<RefCell<Option<Value>>>;

/// A fresh uninitialized top-level cell (seeded at the module top so the NAME
/// resolves even before the declaration runs).
pub fn top_cell() -> TopCell {
    Rc::new(RefCell::new(None))
}

/// Fill a top-level cell when its declaration executes (positional binding).
pub fn top_cell_set(cell: &TopCell, value: Value) {
    *cell.borrow_mut() = Some(value);
}

/// Read a top-level cell. An initialized cell returns its value; an unfilled cell
/// (a forward reference reached before the declaration ran) faults
/// `GUARD_UNBOUND` at the reading identifier's span — byte-identical to the
/// interpreter's positional use-before-binding fault.
pub fn top_cell_get(cell: &TopCell, name: &str, span: Span) -> Result<Value, RtError> {
    match &*cell.borrow() {
        Some(value) => Ok(value.clone()),
        None => Err(fault(
            codes::GUARD_UNBOUND,
            format!("`{name}` is not bound"),
            span,
        )),
    }
}

/// Read a non-entry module export while building its namespace record.
/// Generated modules fill every exported cell first; an incomplete module image
/// returns the ordinary unbound-name fault instead of changing behavior between
/// debug and release builds.
pub fn top_cell_value(cell: &TopCell, name: &str) -> Result<Value, RtError> {
    top_cell_get(cell, name, Span::new(FileId(0), 0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_top_cell_fails_closed_until_initialized() {
        let cell = top_cell();
        let error = top_cell_value(&cell, "answer").expect_err("uninitialized export");
        assert_eq!(error.code, codes::GUARD_UNBOUND);
        assert_eq!(error.message, "`answer` is not bound");

        top_cell_set(&cell, Value::Unit);
        assert!(matches!(top_cell_value(&cell, "answer"), Ok(Value::Unit)));
    }
}
