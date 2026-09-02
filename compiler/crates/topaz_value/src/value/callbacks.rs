use super::*;

pub(super) enum CallbackHofReadyState {
    Map {
        callback: Value,
        remaining: std::vec::IntoIter<Value>,
        values: Vec<Value>,
    },
    Filter {
        callback: Value,
        remaining: std::vec::IntoIter<Value>,
        values: Vec<Value>,
    },
    Reduce {
        callback: Value,
        remaining: std::vec::IntoIter<Value>,
        accumulator: Value,
    },
}

pub(super) enum CallbackHofPendingState {
    Map {
        callback: Value,
        remaining: std::vec::IntoIter<Value>,
        values: Vec<Value>,
    },
    Filter {
        callback: Value,
        remaining: std::vec::IntoIter<Value>,
        values: Vec<Value>,
        item: Value,
    },
    Reduce {
        callback: Value,
        remaining: std::vec::IntoIter<Value>,
    },
}

/// A ready callback-driven `map`/`filter`/`reduce` execution. The evaluator owns
/// callback invocation; this state owns iteration order and result assembly.
pub struct CallbackHofExecution(CallbackHofReadyState);

/// The suspended half of a callback-driven HOF step. Resuming it with the
/// callback result produces the next ready execution state.
pub struct CallbackHofPending(CallbackHofPendingState);

/// One evaluator-independent step of a callback-driven HOF execution.
pub enum CallbackHofStep<Pending = CallbackHofPending> {
    Call {
        pending: Pending,
        callee: Value,
        args: Vec<Value>,
    },
    Complete(Value),
}

pub(super) struct CallbackKeyReadyState {
    callback: Value,
    items: Vec<Value>,
    next: usize,
    keys: Vec<Value>,
}

pub(super) struct CallbackKeyPendingState {
    callback: Value,
    items: Vec<Value>,
    next: usize,
    keys: Vec<Value>,
}

/// A ready callback-driven key projection for `sortedBy` or `sortBy`.
/// The evaluator owns callback invocation; this state owns the item snapshot,
/// projection order, and parallel key assembly.
pub struct CallbackKeyCollection(CallbackKeyReadyState);

/// The suspended half of one callback-key projection step.
pub struct CallbackKeyPending(CallbackKeyPendingState);

/// One evaluator-independent step of callback-key collection.
pub enum CallbackKeyStep {
    Call(CallbackKeyPending),
    Complete { items: Vec<Value>, keys: Vec<Value> },
}

pub(super) struct CallbackRetainReadyState {
    callback: Value,
    remaining: std::vec::IntoIter<Value>,
    kept: Vec<Value>,
}

pub(super) struct CallbackRetainPendingState {
    callback: Value,
    remaining: std::vec::IntoIter<Value>,
    kept: Vec<Value>,
}

/// A ready callback-driven `retain` execution. The evaluator owns callback
/// invocation; this state owns snapshot order and kept-item assembly.
pub struct CallbackRetainExecution(CallbackRetainReadyState);

/// The suspended half of one `retain` predicate step.
pub struct CallbackRetainPending(CallbackRetainPendingState);

/// One evaluator-independent step of callback-driven `retain` execution.
pub enum CallbackRetainStep {
    Call(CallbackRetainPending),
    Complete(Vec<Value>),
}

/// Prepare one array `retain` execution over the caller's item snapshot.
pub fn prepare_callback_retain(items: Vec<Value>, callback: Value) -> CallbackRetainExecution {
    CallbackRetainExecution(CallbackRetainReadyState {
        callback,
        remaining: items.into_iter(),
        kept: Vec::new(),
    })
}

impl CallbackRetainExecution {
    /// Produce the next predicate call or the completed retained items.
    pub fn next(self) -> CallbackRetainStep {
        let CallbackRetainReadyState {
            callback,
            remaining,
            kept,
        } = self.0;
        if remaining.as_slice().is_empty() {
            CallbackRetainStep::Complete(kept)
        } else {
            CallbackRetainStep::Call(CallbackRetainPending(CallbackRetainPendingState {
                callback,
                remaining,
                kept,
            }))
        }
    }
}

impl CallbackRetainPending {
    /// Return the callback and current item owned by this suspended step.
    pub fn invocation(&self) -> (Value, Value) {
        (
            self.0.callback.clone(),
            self.0.remaining.as_slice()[0].clone(),
        )
    }

    /// Apply one predicate result through the shared filter guard and resume.
    pub fn resume(self, predicate: Value, span: Span) -> Result<CallbackRetainExecution, RtError> {
        let CallbackRetainPendingState {
            callback,
            mut remaining,
            mut kept,
        } = self.0;
        let item = remaining.next().expect("pending retain item");
        if filter_keep(&predicate, span)? {
            kept.push(item);
        }
        Ok(CallbackRetainExecution(CallbackRetainReadyState {
            callback,
            remaining,
            kept,
        }))
    }
}

pub(super) enum CallbackMapHofReadyState {
    Filter {
        callback: Value,
        remaining: std::vec::IntoIter<(Value, Value)>,
        values: OrderedMap,
    },
    MapValues {
        callback: Value,
        remaining: std::vec::IntoIter<(Value, Value)>,
        values: OrderedMap,
    },
}

pub(super) enum CallbackMapHofPendingState {
    Filter {
        callback: Value,
        remaining: std::vec::IntoIter<(Value, Value)>,
        values: OrderedMap,
        key: Value,
        value: Value,
    },
    MapValues {
        callback: Value,
        remaining: std::vec::IntoIter<(Value, Value)>,
        values: OrderedMap,
        key: Value,
    },
}

/// A ready callback-driven `Map.filter` or `mapValues` execution. The evaluator
/// owns callback invocation; this state owns pair order and result assembly.
pub struct CallbackMapHofExecution(CallbackMapHofReadyState);

/// The suspended half of one callback-driven map operation.
pub struct CallbackMapHofPending(CallbackMapHofPendingState);

/// One evaluator-independent map callback step using the shared HOF
/// transition protocol with map-specific pending state.
pub type CallbackMapHofStep = CallbackHofStep<CallbackMapHofPending>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackMapHofKind {
    Filter,
    MapValues,
}

/// Prepare one map callback execution over an insertion-order pair snapshot.
pub fn prepare_callback_map_hof(
    kind: CallbackMapHofKind,
    pairs: Vec<(Value, Value)>,
    callback: Value,
) -> CallbackMapHofExecution {
    let remaining = pairs.into_iter();
    let state = match kind {
        CallbackMapHofKind::Filter => CallbackMapHofReadyState::Filter {
            callback,
            remaining,
            values: OrderedMap::new(),
        },
        CallbackMapHofKind::MapValues => CallbackMapHofReadyState::MapValues {
            callback,
            remaining,
            values: OrderedMap::new(),
        },
    };
    CallbackMapHofExecution(state)
}

impl CallbackMapHofExecution {
    /// Produce the next callback request or the completed ordered map.
    pub fn next(self) -> CallbackMapHofStep {
        match self.0 {
            CallbackMapHofReadyState::Filter {
                callback,
                mut remaining,
                values,
            } => match remaining.next() {
                Some((key, value)) => CallbackMapHofStep::Call {
                    callee: callback.clone(),
                    args: vec![key.clone(), value.clone()],
                    pending: CallbackMapHofPending(CallbackMapHofPendingState::Filter {
                        callback,
                        remaining,
                        values,
                        key,
                        value,
                    }),
                },
                None => CallbackMapHofStep::Complete(Value::Map(Rc::new(RefCell::new(values)))),
            },
            CallbackMapHofReadyState::MapValues {
                callback,
                mut remaining,
                values,
            } => match remaining.next() {
                Some((key, value)) => CallbackMapHofStep::Call {
                    callee: callback.clone(),
                    args: vec![value],
                    pending: CallbackMapHofPending(CallbackMapHofPendingState::MapValues {
                        callback,
                        remaining,
                        values,
                        key,
                    }),
                },
                None => CallbackMapHofStep::Complete(Value::Map(Rc::new(RefCell::new(values)))),
            },
        }
    }
}

impl CallbackMapHofPending {
    /// Fold one callback result into the ordered output map and resume.
    pub fn resume(self, result: Value, span: Span) -> Result<CallbackMapHofExecution, RtError> {
        let state = match self.0 {
            CallbackMapHofPendingState::Filter {
                callback,
                remaining,
                mut values,
                key,
                value,
            } => {
                if filter_keep(&result, span)? {
                    values
                        .insert_value(&key, value)
                        .map_err(|error| cmp_guard(error, span))?;
                }
                CallbackMapHofReadyState::Filter {
                    callback,
                    remaining,
                    values,
                }
            }
            CallbackMapHofPendingState::MapValues {
                callback,
                remaining,
                mut values,
                key,
            } => {
                values
                    .insert_value(&key, result)
                    .map_err(|error| cmp_guard(error, span))?;
                CallbackMapHofReadyState::MapValues {
                    callback,
                    remaining,
                    values,
                }
            }
        };
        Ok(CallbackMapHofExecution(state))
    }
}

/// The suspended present-key half of one `Map.update` execution.
pub struct CallbackMapUpdatePending {
    map: Rc<RefCell<OrderedMap>>,
    key: Value,
}

/// One evaluator-independent `Map.update` transition. An absent key completes
/// immediately after inserting `initial`; a present key requests `f(existing)`.
pub enum CallbackMapUpdateStep {
    Call {
        pending: CallbackMapUpdatePending,
        callee: Value,
        existing: Value,
    },
    Complete(Value),
}

/// Probe and prepare one `Map.update(key, initial, f)` transition.
pub fn prepare_callback_map_update(
    map: Rc<RefCell<OrderedMap>>,
    key: Value,
    initial: Value,
    callback: Value,
    span: Span,
) -> Result<CallbackMapUpdateStep, RtError> {
    let existing = map
        .borrow()
        .get_value(&key)
        .map_err(|error| cmp_guard(error, span))?;
    match existing {
        Some(existing) => Ok(CallbackMapUpdateStep::Call {
            pending: CallbackMapUpdatePending { map, key },
            callee: callback,
            existing,
        }),
        None => {
            map.borrow_mut()
                .insert_value(&key, initial)
                .map_err(|error| cmp_guard(error, span))?;
            Ok(CallbackMapUpdateStep::Complete(Value::Unit))
        }
    }
}

impl CallbackMapUpdatePending {
    /// Commit the present-key callback result at the existing insertion slot.
    pub fn resume(self, result: Value, span: Span) -> Result<Value, RtError> {
        self.map
            .borrow_mut()
            .insert_value(&self.key, result)
            .map_err(|error| cmp_guard(error, span))?;
        Ok(Value::Unit)
    }
}

/// The suspended callback half of an `Option.okOrElse` execution.
pub struct CallbackOkOrElsePending;

/// One evaluator-independent `Option.okOrElse` transition. `Some` completes as
/// `Ok` without a callback, while `None` requests one zero-argument callback and
/// wraps its result in `Err`.
pub enum CallbackOkOrElseStep {
    Complete(Value),
    Call {
        pending: CallbackOkOrElsePending,
        callee: Value,
    },
    Unsupported {
        receiver: Value,
    },
}

pub fn prepare_callback_ok_or_else(receiver: Value, callback: Value) -> CallbackOkOrElseStep {
    match receiver {
        Value::Some(value) => CallbackOkOrElseStep::Complete(Value::Ok(value)),
        Value::None => CallbackOkOrElseStep::Call {
            pending: CallbackOkOrElsePending,
            callee: callback,
        },
        receiver => CallbackOkOrElseStep::Unsupported { receiver },
    }
}

impl CallbackOkOrElsePending {
    pub fn resume(self, error: Value) -> Value {
        Value::Err(Rc::new(error))
    }
}

pub(super) enum CallbackReceiverMapWrap {
    Some,
    Ok,
    Identity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackReceiverMapKind {
    Map,
    FlatMap,
}

/// The suspended callback half of an `Option.map` or `Result.map` execution.
pub struct CallbackReceiverMapPending(CallbackReceiverMapWrap);

/// One evaluator-independent receiver `map` transition. Option and Result
/// branches either complete without a callback or request one wrapped result;
/// every other receiver delegates to the iterable callback-HOF authority.
pub enum CallbackReceiverMapStep {
    Call {
        pending: CallbackReceiverMapPending,
        callee: Value,
        input: Value,
    },
    Complete(Value),
    Delegate {
        receiver: Value,
        callback: Value,
    },
    Unsupported {
        receiver: Value,
    },
}

pub fn prepare_callback_receiver_map(receiver: Value, callback: Value) -> CallbackReceiverMapStep {
    prepare_callback_receiver_map_kind(CallbackReceiverMapKind::Map, receiver, callback)
}

pub fn prepare_callback_receiver_flat_map(
    receiver: Value,
    callback: Value,
) -> CallbackReceiverMapStep {
    prepare_callback_receiver_map_kind(CallbackReceiverMapKind::FlatMap, receiver, callback)
}

pub fn prepare_callback_receiver_map_kind(
    kind: CallbackReceiverMapKind,
    receiver: Value,
    callback: Value,
) -> CallbackReceiverMapStep {
    match receiver {
        Value::Some(value) => CallbackReceiverMapStep::Call {
            pending: CallbackReceiverMapPending(if kind == CallbackReceiverMapKind::Map {
                CallbackReceiverMapWrap::Some
            } else {
                CallbackReceiverMapWrap::Identity
            }),
            callee: callback,
            input: (*value).clone(),
        },
        Value::None => CallbackReceiverMapStep::Complete(Value::None),
        Value::Ok(value) => CallbackReceiverMapStep::Call {
            pending: CallbackReceiverMapPending(if kind == CallbackReceiverMapKind::Map {
                CallbackReceiverMapWrap::Ok
            } else {
                CallbackReceiverMapWrap::Identity
            }),
            callee: callback,
            input: (*value).clone(),
        },
        Value::Err(error) => CallbackReceiverMapStep::Complete(Value::Err(error)),
        receiver if kind == CallbackReceiverMapKind::Map => {
            CallbackReceiverMapStep::Delegate { receiver, callback }
        }
        receiver => CallbackReceiverMapStep::Unsupported { receiver },
    }
}

impl CallbackReceiverMapPending {
    pub fn resume(self, result: Value) -> Value {
        match self.0 {
            CallbackReceiverMapWrap::Some => Value::Some(Rc::new(result)),
            CallbackReceiverMapWrap::Ok => Value::Ok(Rc::new(result)),
            CallbackReceiverMapWrap::Identity => result,
        }
    }
}

/// Prepare the shared element-to-key projection used by `sortedBy` and
/// `sortBy`. Consumers choose whether the final stable sort returns a new array
/// or writes into the receiver cell.
pub fn prepare_callback_key_collection(
    items: Vec<Value>,
    callback: Value,
) -> CallbackKeyCollection {
    let capacity = items.len();
    CallbackKeyCollection(CallbackKeyReadyState {
        callback,
        items,
        next: 0,
        keys: Vec::with_capacity(capacity),
    })
}

impl CallbackKeyCollection {
    /// Produce the next key callback or the completed parallel item/key vectors.
    pub fn next(self) -> CallbackKeyStep {
        let CallbackKeyReadyState {
            callback,
            items,
            next,
            keys,
        } = self.0;
        match items.get(next) {
            Some(_) => CallbackKeyStep::Call(CallbackKeyPending(CallbackKeyPendingState {
                callback,
                items,
                next: next + 1,
                keys,
            })),
            None => CallbackKeyStep::Complete { items, keys },
        }
    }
}

impl CallbackKeyPending {
    /// Return the callback and current item owned by this suspended step.
    pub fn invocation(&self) -> (Value, Value) {
        (
            self.0.callback.clone(),
            self.0.items[self.0.next - 1].clone(),
        )
    }

    /// Append one projected key and resume the shared collection state.
    pub fn resume(self, key: Value) -> CallbackKeyCollection {
        let CallbackKeyPendingState {
            callback,
            items,
            next,
            mut keys,
        } = self.0;
        keys.push(key);
        CallbackKeyCollection(CallbackKeyReadyState {
            callback,
            items,
            next,
            keys,
        })
    }
}

/// The three callback-driven collection operations accepted by the shared HOF
/// state machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackHofKind {
    Map,
    Filter,
    Reduce,
}

impl CallbackHofKind {
    pub fn from_builtin(kind: Builtin) -> Option<Self> {
        match kind {
            Builtin::MapFn => Some(Self::Map),
            Builtin::FilterFn => Some(Self::Filter),
            Builtin::ReduceFn => Some(Self::Reduce),
            _ => None,
        }
    }
}

/// Prepare one free or array-receiver `map`/`filter`/`reduce` call.
pub fn prepare_callback_hof(
    kind: CallbackHofKind,
    args: Vec<Value>,
    span: Span,
) -> Result<CallbackHofExecution, RtError> {
    match kind {
        CallbackHofKind::Map | CallbackHofKind::Filter => {
            let [iterable, callback] = exact_args(args, span)?;
            let remaining = iterable_items(iterable, span)?.into_iter();
            let state = if kind == CallbackHofKind::Map {
                CallbackHofReadyState::Map {
                    callback,
                    remaining,
                    values: Vec::new(),
                }
            } else {
                CallbackHofReadyState::Filter {
                    callback,
                    remaining,
                    values: Vec::new(),
                }
            };
            Ok(CallbackHofExecution(state))
        }
        CallbackHofKind::Reduce => {
            let [iterable, accumulator, callback] = exact_args(args, span)?;
            let remaining = iterable_items(iterable, span)?.into_iter();
            Ok(CallbackHofExecution(CallbackHofReadyState::Reduce {
                callback,
                remaining,
                accumulator,
            }))
        }
    }
}

impl CallbackHofExecution {
    /// Produce the next callback invocation or the completed HOF value.
    pub fn next(self) -> CallbackHofStep {
        match self.0 {
            CallbackHofReadyState::Map {
                callback,
                mut remaining,
                values,
            } => match remaining.next() {
                Some(item) => CallbackHofStep::Call {
                    callee: callback.clone(),
                    args: vec![item],
                    pending: CallbackHofPending(CallbackHofPendingState::Map {
                        callback,
                        remaining,
                        values,
                    }),
                },
                None => CallbackHofStep::Complete(Value::array(values)),
            },
            CallbackHofReadyState::Filter {
                callback,
                mut remaining,
                values,
            } => match remaining.next() {
                Some(item) => CallbackHofStep::Call {
                    callee: callback.clone(),
                    args: vec![item.clone()],
                    pending: CallbackHofPending(CallbackHofPendingState::Filter {
                        callback,
                        remaining,
                        values,
                        item,
                    }),
                },
                None => CallbackHofStep::Complete(Value::array(values)),
            },
            CallbackHofReadyState::Reduce {
                callback,
                mut remaining,
                accumulator,
            } => match remaining.next() {
                Some(item) => CallbackHofStep::Call {
                    callee: callback.clone(),
                    args: vec![accumulator, item],
                    pending: CallbackHofPending(CallbackHofPendingState::Reduce {
                        callback,
                        remaining,
                    }),
                },
                None => CallbackHofStep::Complete(accumulator),
            },
        }
    }
}

impl CallbackHofPending {
    /// Fold one callback result into the shared execution state.
    pub fn resume(self, result: Value, span: Span) -> Result<CallbackHofExecution, RtError> {
        Ok(CallbackHofExecution(match self.0 {
            CallbackHofPendingState::Map {
                callback,
                remaining,
                mut values,
            } => {
                values.push(result);
                CallbackHofReadyState::Map {
                    callback,
                    remaining,
                    values,
                }
            }
            CallbackHofPendingState::Filter {
                callback,
                remaining,
                mut values,
                item,
            } => {
                if filter_keep(&result, span)? {
                    values.push(item);
                }
                CallbackHofReadyState::Filter {
                    callback,
                    remaining,
                    values,
                }
            }
            CallbackHofPendingState::Reduce {
                callback,
                remaining,
            } => CallbackHofReadyState::Reduce {
                callback,
                remaining,
                accumulator: result,
            },
        }))
    }
}
