use super::model::*;
use crate::*;

pub(crate) type Slot = Rc<RefCell<RuntimeValue>>;
pub(crate) type Environment = Rc<EnvironmentFrame>;

pub(crate) struct EnvironmentFrame {
    pub(crate) values: RefCell<BTreeMap<String, Slot>>,
    pub(crate) parent: Option<Environment>,
}

impl EnvironmentFrame {
    pub(crate) fn root() -> Environment {
        Rc::new(Self {
            values: RefCell::new(BTreeMap::new()),
            parent: None,
        })
    }

    pub(crate) fn child(parent: Environment) -> Environment {
        Rc::new(Self {
            values: RefCell::new(BTreeMap::new()),
            parent: Some(parent),
        })
    }

    pub(crate) fn define(&self, key: String, value: RuntimeValue) {
        self.values
            .borrow_mut()
            .insert(key, Rc::new(RefCell::new(value)));
    }

    pub(crate) fn slot(&self, key: &str) -> Option<Slot> {
        self.values
            .borrow()
            .get(key)
            .cloned()
            .or_else(|| self.parent.as_ref()?.slot(key))
    }
}

pub(crate) enum Flow {
    Value(RuntimeValue),
    Return(RuntimeValue),
    Break { target: String, value: RuntimeValue },
    Continue { target: String },
}
