//! Cells for top-level function references in generated programs.
//! A declaration initializes its cell when execution reaches that declaration.
//! Reading an uninitialized cell returns TPZ5002 at the identifier's span.
//! Option distinguishes an uninitialized cell from a stored Unit value.

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
