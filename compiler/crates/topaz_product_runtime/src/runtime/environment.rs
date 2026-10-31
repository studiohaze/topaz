use super::model::*;
use crate::*;

pub(crate) type Slot = Rc<RefCell<RuntimeValue>>;
pub(crate) type Environment = Rc<EnvironmentFrame>;

pub(crate) struct EnvironmentFrame {
    pub(crate) values: RefCell<BTreeMap<String, Slot>>,
    /// Declarations keep an operation index, not a closure owning this frame.
    /// A lookup captures the declaration's frame when the function is used.
    functions: RefCell<BTreeMap<String, FunctionBinding>>,
    /// Const expressions may name a declaration before its statement executes.
    declaration_reads: bool,
    /// One bit per name class defined in this frame. A clear bit proves the
    /// frame cannot hold the name, so a lookup that misses walks the chain
    /// without searching each frame's map.
    defined: Cell<u64>,
    pub(crate) parent: Option<Environment>,
}

struct FunctionBinding {
    operation: usize,
    name: Rc<str>,
    available: bool,
}

/// Maps a name to its bit in a frame's membership word. Reads the ends of the
/// name so the cost does not grow with qualified identities.
fn name_bit(key: &str) -> u64 {
    let bytes = key.as_bytes();
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ (bytes.len() as u64);
    let head = bytes.iter().take(6);
    let tail = bytes.iter().rev().take(6);
    for byte in head.chain(tail) {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    1_u64 << ((hash >> 24) % 64)
}

impl EnvironmentFrame {
    pub(crate) fn root() -> Environment {
        Rc::new(Self {
            values: RefCell::new(BTreeMap::new()),
            functions: RefCell::new(BTreeMap::new()),
            declaration_reads: false,
            defined: Cell::new(0),
            parent: None,
        })
    }

    pub(crate) fn child(parent: Environment) -> Environment {
        Rc::new(Self {
            values: RefCell::new(BTreeMap::new()),
            functions: RefCell::new(BTreeMap::new()),
            declaration_reads: false,
            defined: Cell::new(0),
            parent: Some(parent),
        })
    }

    pub(crate) fn for_constants(parent: Environment) -> Environment {
        Rc::new(Self {
            values: RefCell::new(BTreeMap::new()),
            functions: RefCell::new(BTreeMap::new()),
            declaration_reads: true,
            defined: Cell::new(0),
            parent: Some(parent),
        })
    }

    pub(crate) fn define(&self, key: String, value: RuntimeValue) {
        self.defined.set(self.defined.get() | name_bit(&key));
        self.values
            .borrow_mut()
            .insert(key, Rc::new(RefCell::new(value)));
    }

    /// A constant alias of a function declared in this frame must not own the
    /// frame. Reuse the operation slot; lookup captures the frame on demand.
    /// Functions from other activations retain their captured environment.
    pub(crate) fn define_constant(self: &Rc<Self>, key: String, value: RuntimeValue) {
        if let RuntimeValue::Function {
            operation,
            environment,
            name: Some(name),
        } = &value
            && Rc::ptr_eq(self, environment)
        {
            self.define_function(key, *operation, name.clone());
        } else {
            self.define(key, value);
        }
    }

    /// Empties the frame so a caller that still solely owns it can bind into
    /// it again.
    pub(crate) fn reset(&self) {
        self.values.borrow_mut().clear();
        self.functions.borrow_mut().clear();
        self.defined.set(0);
    }

    pub(crate) fn define_function(&self, key: String, operation: usize, name: Rc<str>) {
        self.defined.set(self.defined.get() | name_bit(&key));
        self.functions.borrow_mut().insert(
            key,
            FunctionBinding {
                operation,
                name,
                available: true,
            },
        );
    }

    pub(crate) fn reserve_function(&self, key: String, operation: usize, name: Rc<str>) {
        self.defined.set(self.defined.get() | name_bit(&key));
        self.functions
            .borrow_mut()
            .entry(key)
            .or_insert(FunctionBinding {
                operation,
                name,
                available: false,
            });
    }

    pub(crate) fn has_function(&self, keys: &[&str]) -> bool {
        let mut frame = self;
        loop {
            if keys.iter().any(|key| {
                frame.defined.get() & name_bit(key) != 0
                    && frame.functions.borrow().contains_key(*key)
            }) {
                return true;
            }
            let Some(parent) = frame.parent.as_deref() else {
                return false;
            };
            frame = parent;
        }
    }

    pub(crate) fn lookup(self: &Rc<Self>, keys: &[&str]) -> Option<RuntimeValue> {
        let mut frame = self;
        loop {
            for key in keys.iter().copied().filter(|key| !key.is_empty()) {
                if frame.defined.get() & name_bit(key) == 0 {
                    continue;
                }
                if let Some(slot) = frame.values.borrow().get(key) {
                    return Some(slot.borrow().clone());
                }
                if let Some(function) = frame.functions.borrow().get(key)
                    && (function.available || self.declaration_reads)
                {
                    return Some(RuntimeValue::Function {
                        operation: function.operation,
                        environment: frame.clone(),
                        name: Some(function.name.clone()),
                    });
                }
            }
            frame = frame.parent.as_ref()?;
        }
    }

    pub(crate) fn slot_any(&self, keys: &[&str]) -> Option<Slot> {
        let mut frame = self;
        loop {
            for key in keys.iter().copied().filter(|key| !key.is_empty()) {
                if frame.defined.get() & name_bit(key) != 0
                    && let Some(slot) = frame.values.borrow().get(key)
                {
                    return Some(slot.clone());
                }
            }
            frame = frame.parent.as_deref()?;
        }
    }

    pub(crate) fn slot(&self, key: &str) -> Option<Slot> {
        let bit = name_bit(key);
        let mut frame = self;
        loop {
            if frame.defined.get() & bit != 0
                && let Some(slot) = frame.values.borrow().get(key)
            {
                return Some(slot.clone());
            }
            frame = frame.parent.as_deref()?;
        }
    }
}

pub(crate) enum Flow {
    Value(RuntimeValue),
    Return(RuntimeValue),
    Break { target: String, value: RuntimeValue },
    Continue { target: String },
}
