use super::*;
use std::cell::{Ref, RefMut};

/// Element tests whose result cannot change through a nested mutable value.
/// Structural and generic nominal tests still walk their complete contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StableArrayElement {
    Int,
    Float,
    Bool,
    String,
    Unit,
    Null,
    NominalRecord(&'static str),
    Enum(&'static str),
    Newtype(&'static str),
}

impl StableArrayElement {
    fn matches(self, value: &Value) -> bool {
        match self {
            Self::Int => matches!(value, Value::Int(_)),
            Self::Float => matches!(value, Value::Float(_)),
            Self::Bool => matches!(value, Value::Bool(_)),
            Self::String => matches!(value, Value::Str(_)),
            Self::Unit => matches!(value, Value::Unit),
            Self::Null => matches!(value, Value::Null),
            Self::NominalRecord(id) => value.is_nominal_record_declaration(id),
            Self::Newtype(id) => value.is_newtype_declaration(id),
            Self::Enum(id) => matches!(value, Value::Enum { enum_id, declaration_identity, .. }
                if nominal_declaration_identity(enum_id, declaration_identity.as_deref()) == id),
        }
    }
}

/// Shared array storage owns both mutations and reusable element checks.
/// A general mutable borrow invalidates every check before exposing the vector.
/// Appending preserves the checked prefix; the next guard checks the new suffix.
#[derive(Debug, Default)]
pub struct ArrayValue {
    items: RefCell<Vec<Value>>,
    checked: RefCell<Vec<(StableArrayElement, usize)>>,
}

impl ArrayValue {
    pub fn new(items: Vec<Value>) -> Self {
        Self {
            items: RefCell::new(items),
            checked: RefCell::new(Vec::new()),
        }
    }

    pub fn borrow(&self) -> Ref<'_, Vec<Value>> {
        self.items.borrow()
    }

    pub fn borrow_mut(&self) -> RefMut<'_, Vec<Value>> {
        let items = self.items.borrow_mut();
        self.checked.borrow_mut().clear();
        items
    }

    pub fn push(&self, value: Value) {
        self.items.borrow_mut().push(value);
    }

    pub fn elements_match(&self, element: StableArrayElement) -> bool {
        let items = self.items.borrow();
        let mut checked = self.checked.borrow_mut();
        let position = checked.iter().position(|(kind, _)| *kind == element);
        let start = position.map_or(0, |index| checked[index].1);
        if !items[start..].iter().all(|value| element.matches(value)) {
            return false;
        }
        if let Some(index) = position {
            checked[index].1 = items.len();
        } else {
            // Keep memory bounded even when an empty array crosses many types.
            if checked.len() == 8 {
                checked.remove(0);
            }
            checked.push((element, items.len()));
        }
        true
    }
}

pub fn array_elements_match(value: &Value, element: StableArrayElement) -> bool {
    matches!(value, Value::Array(items) if items.elements_match(element))
}
