use super::*;

/// Canonical key: a frozen deep snapshot of a comparable value
/// (CDR-003 §2). Keys contain no shared state and no cycles.
#[derive(Debug, Clone)]
pub enum Key {
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Bool(bool),
    Unit,
    Null,
    Some(Box<Key>),
    None,
    Ok(Box<Key>),
    Err(Box<Key>),
    Array(Vec<Key>),
    Record(BTreeMap<String, Key>),
    /// §3 (v5.4) a user enum Map/Set key: enum identity plus variant payload keys.
    Enum {
        enum_id: Rc<str>,
        declaration_identity: Option<Rc<str>>,
        method_identity: Option<Rc<str>>,
        variant: Rc<str>,
        variant_index: u32,
        payloads: Vec<Key>,
    },
    /// §3 (v5.4) a nominal record Map/Set key: identity plus decl-ordered fields.
    NominalRecord {
        record_id: Rc<str>,
        declaration_identity: Option<Rc<str>>,
        method_identity: Option<Rc<str>>,
        fields: Vec<(Rc<str>, Key)>,
    },
    /// §8 (v5.4) a `Bytes` Map/Set key: an immutable byte array IS keyable (no
    /// shared mutable state, like an `int`/`string` key — its canonical form is
    /// just the bytes). Frozen + thawed by IDENTITY (the `Rc<[u8]>` is shared, not
    /// deep-copied, since it is already immutable).
    Bytes(Rc<[u8]>),
    /// §10 (v5.4) a `Path` Map/Set key: immutable + normalized string payload.
    Path(Rc<str>),
    /// §16 (v5.4) a `URL` Map/Set key: immutable parsed value, canonical-string identity.
    Url(Rc<UrlData>),
    /// §13 (v5.4) a `Date` Map/Set key: immutable day count.
    Date(DateData),
    /// §14.1 (v5.4) a `BigInt` Map/Set key: immutable arbitrary-precision integer.
    BigInt(Rc<BigIntData>),
    /// §14.2 (v5.4) a `Decimal` Map/Set key: immutable canonical exact decimal.
    Decimal(Rc<DecimalData>),
    /// §3 (v5.4) a newtype Map/Set key: identity plus the frozen base key.
    Newtype {
        newtype_id: Rc<str>,
        declaration_identity: Option<Rc<str>>,
        method_identity: Option<Rc<str>>,
        inner: Box<Key>,
    },
}

/// Deep-snapshot a value into a canonical key; comparability is
/// enforced here (§2/§9 — every §22.2 key site routes through this).
pub fn canonical_key(value: &Value) -> Result<Key, CmpError> {
    let mut fuel = STRUCT_FUEL;
    freeze(value, &mut fuel, 0)
}

pub(super) fn freeze(value: &Value, fuel: &mut usize, depth: usize) -> Result<Key, CmpError> {
    if *fuel == 0 || depth > STRUCT_DEPTH {
        return Err(CmpError::Fuel);
    }
    *fuel -= 1;
    Ok(match value {
        Value::Int(x) => Key::Int(*x),
        Value::Float(x) => Key::Float(*x),
        Value::Str(s) => Key::Str(s.clone()),
        // §8/§10 (v5.4) `Bytes`/`Path` are keyable immutable scalar-like leaves.
        Value::Bytes(b) => Key::Bytes(b.clone()),
        Value::ByteBuffer(_) => return Err(CmpError::NotComparable("ByteBuffer")),
        Value::Path(p) => Key::Path(p.clone()),
        Value::Url(u) => Key::Url(u.clone()),
        Value::Date(d) => Key::Date(*d),
        Value::BigInt(n) => Key::BigInt(n.clone()),
        Value::Decimal(d) => Key::Decimal(d.clone()),
        Value::Regex(_) => return Err(CmpError::NotComparable("Regex")),
        Value::RegexMatch(_) => return Err(CmpError::NotComparable("Match")),
        Value::Toml(_) => return Err(CmpError::NotComparable("TOMLValue")),
        Value::Bool(b) => Key::Bool(*b),
        Value::Unit => Key::Unit,
        Value::Null => Key::Null,
        Value::Some(v) => Key::Some(Box::new(freeze(v, fuel, depth + 1)?)),
        Value::None => Key::None,
        Value::Ok(v) => Key::Ok(Box::new(freeze(v, fuel, depth + 1)?)),
        Value::Err(v) => Key::Err(Box::new(freeze(v, fuel, depth + 1)?)),
        Value::Array(items) => Key::Array(
            items
                .borrow()
                .iter()
                .map(|v| freeze(v, fuel, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        Value::Record(fields) => Key::Record(
            fields
                .iter()
                .map(|(name, v)| Ok((name.clone(), freeze(v, fuel, depth + 1)?)))
                .collect::<Result<_, _>>()?,
        ),
        Value::Map(_) => return Err(CmpError::NotComparable("Map")),
        Value::Set(_) => return Err(CmpError::NotComparable("Set")),
        Value::Resource(_) => return Err(CmpError::NotComparable("File")),
        Value::Closure(_) => return Err(CmpError::NotComparable("function")),
        Value::Builtin { .. } => return Err(CmpError::NotComparable("function")),
        Value::LispexApplicationOpaque(value) => {
            return Err(CmpError::NotComparable(value.kind_name()));
        }
        Value::Namespace(_) => return Err(CmpError::NotComparable("namespace")),
        Value::Template(_) => return Err(CmpError::NotComparable("template")),
        Value::Composed(_) => return Err(CmpError::NotComparable("function")),
        Value::Range { .. } => return Err(CmpError::NotComparable("range")),
        Value::Json(_) => return Err(CmpError::NotComparable("JSONValue")),
        Value::Enum { enum_id, .. } if enum_id.as_ref() == "RoundingMode" => {
            return Err(CmpError::NotComparable("RoundingMode"));
        }
        Value::Enum {
            enum_id,
            declaration_identity,
            method_identity,
            variant,
            variant_index,
            payloads,
        } => Key::Enum {
            enum_id: enum_id.clone(),
            declaration_identity: declaration_identity.clone(),
            method_identity: method_identity.clone(),
            variant: variant.clone(),
            variant_index: *variant_index,
            payloads: payloads
                .iter()
                .map(|value| freeze(value, fuel, depth + 1))
                .collect::<Result<_, _>>()?,
        },
        Value::NominalRecord {
            record_id,
            declaration_identity,
            method_identity,
            fields,
        } => Key::NominalRecord {
            record_id: record_id.clone(),
            declaration_identity: declaration_identity.clone(),
            method_identity: method_identity.clone(),
            fields: fields
                .iter()
                .map(|(name, value)| Ok((name.clone(), freeze(value, fuel, depth + 1)?)))
                .collect::<Result<_, _>>()?,
        },
        Value::Newtype {
            newtype_id,
            declaration_identity,
            method_identity,
            inner,
        } => Key::Newtype {
            newtype_id: newtype_id.clone(),
            declaration_identity: declaration_identity.clone(),
            method_identity: method_identity.clone(),
            inner: Box::new(freeze(inner, fuel, depth + 1)?),
        },
    })
}

/// Thaw a key back into a value (`m.keys` snapshots, §22.2).
pub fn key_to_value(key: &Key) -> Value {
    match key {
        Key::Int(x) => Value::Int(*x),
        Key::Float(x) => Value::Float(*x),
        Key::Str(s) => Value::Str(s.clone()),
        Key::Bytes(b) => Value::Bytes(b.clone()),
        Key::Path(p) => Value::Path(p.clone()),
        Key::Url(u) => Value::Url(u.clone()),
        Key::Date(d) => Value::Date(*d),
        Key::BigInt(n) => Value::BigInt(n.clone()),
        Key::Decimal(d) => Value::Decimal(d.clone()),
        Key::Bool(b) => Value::Bool(*b),
        Key::Unit => Value::Unit,
        Key::Null => Value::Null,
        Key::Some(k) => Value::Some(Rc::new(key_to_value(k))),
        Key::None => Value::None,
        Key::Ok(k) => Value::Ok(Rc::new(key_to_value(k))),
        Key::Err(k) => Value::Err(Rc::new(key_to_value(k))),
        Key::Array(items) => Value::array(items.iter().map(key_to_value).collect()),
        Key::Record(fields) => Value::Record(Rc::new(
            fields
                .iter()
                .map(|(n, k)| (n.clone(), key_to_value(k)))
                .collect(),
        )),
        Key::Enum {
            enum_id,
            declaration_identity,
            method_identity,
            variant,
            variant_index,
            payloads,
        } => {
            let payloads: Vec<Value> = payloads.iter().map(key_to_value).collect();
            Value::Enum {
                enum_id: enum_id.clone(),
                declaration_identity: declaration_identity.clone(),
                method_identity: method_identity.clone(),
                variant: variant.clone(),
                variant_index: *variant_index,
                payloads: Rc::from(payloads.into_boxed_slice()),
            }
        }
        Key::NominalRecord {
            record_id,
            declaration_identity,
            method_identity,
            fields,
        } => {
            let fields: Vec<(Rc<str>, Value)> = fields
                .iter()
                .map(|(name, key)| (name.clone(), key_to_value(key)))
                .collect();
            Value::NominalRecord {
                record_id: record_id.clone(),
                declaration_identity: declaration_identity.clone(),
                method_identity: method_identity.clone(),
                fields: Rc::from(fields.into_boxed_slice()),
            }
        }
        Key::Newtype {
            newtype_id,
            declaration_identity,
            method_identity,
            inner,
        } => Value::Newtype {
            newtype_id: newtype_id.clone(),
            declaration_identity: declaration_identity.clone(),
            method_identity: method_identity.clone(),
            inner: Rc::new(key_to_value(inner)),
        },
    }
}

pub(super) fn keys_equal(a: &Key, b: &Key) -> bool {
    use Key::*;
    match (a, b) {
        (Int(x), Int(y)) => x == y,
        // IEEE: a NaN key can never be found again — permitted.
        (Float(x), Float(y)) => x == y,
        (Str(x), Str(y)) => x == y,
        // §8 (v5.4) two `Bytes` keys are equal iff byte-identical (the SAME byte-wise
        // rule as value `==`), so a `Bytes` round-trips as a Map/Set key.
        (Bytes(x), Bytes(y)) => x == y,
        (Path(x), Path(y)) => x == y,
        (Url(x), Url(y)) => x.canonical.as_ref() == y.canonical.as_ref(),
        (Date(x), Date(y)) => x == y,
        (BigInt(x), BigInt(y)) => x == y,
        (Decimal(x), Decimal(y)) => x == y,
        (Bool(x), Bool(y)) => x == y,
        (Unit, Unit) | (Null, Null) | (None, None) => true,
        (Some(x), Some(y)) | (Ok(x), Ok(y)) | (Err(x), Err(y)) => keys_equal(x, y),
        (Array(x), Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| keys_equal(a, b))
        }
        (Record(x), Record(y)) => {
            x.len() == y.len()
                && x.keys().eq(y.keys())
                && x.values().zip(y.values()).all(|(a, b)| keys_equal(a, b))
        }
        (
            Enum {
                enum_id: ex,
                declaration_identity: dx,
                variant: vx,
                variant_index: ix,
                payloads: px,
                ..
            },
            Enum {
                enum_id: ey,
                declaration_identity: dy,
                variant: vy,
                variant_index: iy,
                payloads: py,
                ..
            },
        ) => {
            nominal_declaration_identity(ex, dx.as_deref())
                == nominal_declaration_identity(ey, dy.as_deref())
                && vx == vy
                && ix == iy
                && px.len() == py.len()
                && px.iter().zip(py.iter()).all(|(kx, ky)| keys_equal(kx, ky))
        }
        (
            NominalRecord {
                record_id: rx,
                declaration_identity: dx,
                fields: fx,
                ..
            },
            NominalRecord {
                record_id: ry,
                declaration_identity: dy,
                fields: fy,
                ..
            },
        ) => {
            nominal_declaration_identity(rx, dx.as_deref())
                == nominal_declaration_identity(ry, dy.as_deref())
                && fx.len() == fy.len()
                && fx
                    .iter()
                    .zip(fy.iter())
                    .all(|((nx, kx), (ny, ky))| nx == ny && keys_equal(kx, ky))
        }
        (
            Newtype {
                newtype_id: nx,
                declaration_identity: dx,
                inner: ix,
                ..
            },
            Newtype {
                newtype_id: ny,
                declaration_identity: dy,
                inner: iy,
                ..
            },
        ) => {
            nominal_declaration_identity(nx, dx.as_deref())
                == nominal_declaration_identity(ny, dy.as_deref())
                && keys_equal(ix, iy)
        }
        _ => false,
    }
}

/// Insertion-ordered map (§22.2: `m.keys` is an insertion-order
/// snapshot). Reference implementation uses linear association —
/// clarity over speed.
#[derive(Debug, Default)]
pub struct OrderedMap {
    pub(super) entries: Vec<(Key, Value)>,
}

impl OrderedMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: Key, value: Value) {
        if let Some(entry) = self.entries.iter_mut().find(|(k, _)| keys_equal(k, &key)) {
            entry.1 = value; // existing key keeps its insertion slot
        } else {
            self.entries.push((key, value));
        }
    }

    /// §6 (v5.4) `map { … }` LITERAL insert. Returns `true` iff the key was newly
    /// inserted; returns `false` (and does NOT overwrite) when the key is ALREADY
    /// present — the caller (`builtin_map_of`) raises TPZ4601 on a `false`. This
    /// is the literal-specific contract: `Map.insert` silently overwrites, but a
    /// literal asserts unique keys.
    pub fn try_insert(&mut self, key: Key, value: Value) -> bool {
        if self.entries.iter().any(|(k, _)| keys_equal(k, &key)) {
            return false;
        }
        self.entries.push((key, value));
        true
    }

    pub fn get(&self, key: &Key) -> Option<Value> {
        self.entries
            .iter()
            .find(|(k, _)| keys_equal(k, key))
            .map(|(_, v)| v.clone())
    }

    pub fn remove(&mut self, key: &Key) -> Option<Value> {
        let i = self.entries.iter().position(|(k, _)| keys_equal(k, key))?;
        Some(self.entries.remove(i).1)
    }

    /// §22.2 checked surface: canonicalize at every key site.
    pub fn insert_value(&mut self, key: &Value, value: Value) -> Result<(), CmpError> {
        self.insert(canonical_key(key)?, value);
        Ok(())
    }

    pub fn get_value(&self, key: &Value) -> Result<Option<Value>, CmpError> {
        Ok(self.get(&canonical_key(key)?))
    }

    pub fn remove_value(&mut self, key: &Value) -> Result<Option<Value>, CmpError> {
        Ok(self.remove(&canonical_key(key)?))
    }

    /// Snapshot of (key, value) pairs in insertion order (§6 runtime
    /// conformance).
    pub fn pairs(&self) -> Vec<(Value, Value)> {
        self.entries
            .iter()
            .map(|(k, v)| (key_to_value(k), v.clone()))
            .collect()
    }

    pub fn keys(&self) -> Vec<Value> {
        self.entries.iter().map(|(k, _)| key_to_value(k)).collect()
    }

    /// Snapshot of values in insertion order (parallels `keys`).
    pub fn values(&self) -> Vec<Value> {
        self.entries.iter().map(|(_, v)| v.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// §6 (v5.4) `m.clear()` — empties the map in place (the `Rc<RefCell>` cell is
    /// shared, so the change reaches the binding).
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Insertion-ordered set (§9/§22.2).
#[derive(Debug, Default)]
pub struct OrderedSet {
    pub(super) items: Vec<Key>,
}

impl OrderedSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true iff the element was newly added.
    pub fn add(&mut self, key: Key) -> bool {
        if self.items.iter().any(|k| keys_equal(k, &key)) {
            return false;
        }
        self.items.push(key);
        true
    }

    pub fn contains(&self, key: &Key) -> bool {
        self.items.iter().any(|k| keys_equal(k, key))
    }

    /// Returns true iff an element was removed (§22.2 `s.remove`).
    pub fn remove(&mut self, key: &Key) -> bool {
        match self.items.iter().position(|k| keys_equal(k, key)) {
            Some(i) => {
                self.items.remove(i);
                true
            }
            None => false,
        }
    }

    /// §22.2 checked surface: canonicalize at every element site.
    /// Snapshot of the elements in insertion order (§6 runtime
    /// conformance).
    pub fn items(&self) -> Vec<Value> {
        self.items.iter().map(key_to_value).collect()
    }

    pub fn add_value(&mut self, item: &Value) -> Result<bool, CmpError> {
        Ok(self.add(canonical_key(item)?))
    }

    pub fn contains_value(&self, item: &Value) -> Result<bool, CmpError> {
        Ok(self.contains(&canonical_key(item)?))
    }

    pub fn remove_value(&mut self, item: &Value) -> Result<bool, CmpError> {
        Ok(self.remove(&canonical_key(item)?))
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// §6 (v5.4) `s.clear()` — empties the set in place (shared cell → reaches the binding).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// §6 (v5.4) set ALGEBRA — each returns a fresh `OrderedSet` with a DETERMINISTIC
    /// insertion-order result. The element type is already keyable (the operands are
    /// valid `Set<T>`s, every element canonicalized at its add site), so cloning the
    /// stored `Key`s needs no re-canonicalization and cannot fault.
    ///
    /// `union` = self's elements (in self's order) THEN other's elements not already
    /// present (in other's order).
    pub fn union(&self, other: &OrderedSet) -> OrderedSet {
        let mut out = OrderedSet {
            items: self.items.clone(),
        };
        for k in &other.items {
            out.add(k.clone());
        }
        out
    }

    /// `intersection` = self's elements that are also in `other`, in SELF's order.
    pub fn intersection(&self, other: &OrderedSet) -> OrderedSet {
        OrderedSet {
            items: self
                .items
                .iter()
                .filter(|k| other.contains(k))
                .cloned()
                .collect(),
        }
    }

    /// `difference` = self's elements NOT in `other`, in SELF's order.
    pub fn difference(&self, other: &OrderedSet) -> OrderedSet {
        OrderedSet {
            items: self
                .items
                .iter()
                .filter(|k| !other.contains(k))
                .cloned()
                .collect(),
        }
    }
}

/// Materialize an iterable value into an ordered snapshot (§10): the
/// shared helper both engines' for-loops and higher-order builtins
/// consume, so iteration order cannot drift between them.
pub fn iterable_items(value: Value, span: Span) -> Result<Vec<Value>, RtError> {
    match &value {
        Value::Array(items) => Ok(items.borrow().clone()),
        Value::Set(set) => Ok(set.borrow().items()),
        Value::Range { .. } => range_items(&value, span),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`{}` is not iterable (§10)", other.kind()),
            span,
        )),
    }
}

/// §10 `for`-iteration materialization — the SINGLE shared element list a
/// `for` loop walks, so the sequence AND the not-iterable faults are
/// identical on both engines (CDR-006 §2). Distinct from
/// [`iterable_items`]: a `for` gives `Map` and `str` their own hint
/// messages (iterate `m.keys` / use `s.scalars()`) and says
/// "not `for`-iterable", matching the interpreter's `KForStart`.
pub fn for_items(value: &Value, span: Span) -> Result<Vec<Value>, RtError> {
    match value {
        Value::Array(items) => Ok(items.borrow().clone()),
        Value::Set(set) => Ok(set.borrow().items()),
        Value::Map(_) => Err(fault(
            codes::GUARD_TYPE,
            "`for` over `Map` is a static error; iterate `m.keys` (§10)",
            span,
        )),
        Value::Range { .. } => range_items(value, span),
        Value::Str(_) => Err(fault(
            codes::GUARD_TYPE,
            "strings are not `for`-iterable; use `s.scalars()` (§10)",
            span,
        )),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`{}` is not `for`-iterable (§10)", other.kind()),
            span,
        )),
    }
}

/// §22.2 `Set.of(items…)` — the SHARED constructor both engines call. It
/// builds an `OrderedSet` (insertion order, deduplicated); a non-hashable
/// item faults through `cmp_guard`, identically on both engines.
pub fn builtin_set_of(args: Vec<Value>, span: Span) -> Result<Value, RtError> {
    let mut set = OrderedSet::new();
    for item in &args {
        set.add_value(item).map_err(|e| cmp_guard(e, span))?;
    }
    Ok(Value::Set(Rc::new(RefCell::new(set))))
}

/// §22.2 `Map.new()` — the SHARED constructor for an empty `OrderedMap`.
pub fn builtin_map_new() -> Value {
    Value::Map(Rc::new(RefCell::new(OrderedMap::new())))
}

/// v5.4 `Map.ofEntries(entries)` - the SHARED constructor both engines call.
/// Entries must be structural records `{ key, value }`; duplicate keys follow
/// `Map.insert` semantics (later value wins, original key slot is kept).
pub fn builtin_map_of_entries(entries: Value, span: Span) -> Result<Value, RtError> {
    let items = match entries {
        Value::Array(items) => items,
        other => {
            return Err(fault(
                codes::GUARD_TYPE,
                format!(
                    "`Map.ofEntries` takes an `Array` of `{{ key, value }}` records, found `{}`",
                    other.kind()
                ),
                span,
            ));
        }
    };
    let mut out = OrderedMap::new();
    for entry in items.borrow().iter() {
        let Value::Record(fields) = entry else {
            return Err(fault(
                codes::GUARD_TYPE,
                format!(
                    "`Map.ofEntries` entries must be records `{{ key, value }}`, found `{}`",
                    entry.kind()
                ),
                span,
            ));
        };
        let (Some(key), Some(value)) = (fields.get("key"), fields.get("value")) else {
            return Err(fault(
                codes::GUARD_TYPE,
                "`Map.ofEntries` entries must have `key` and `value` fields",
                span,
            ));
        };
        if fields.len() != 2 {
            return Err(fault(
                codes::GUARD_TYPE,
                "`Map.ofEntries` entries must have exactly `key` and `value` fields",
                span,
            ));
        }
        out.insert_value(key, value.clone())
            .map_err(|e| cmp_guard(e, span))?;
    }
    Ok(Value::Map(Rc::new(RefCell::new(out))))
}

/// §6 (v5.4) `map { k: v, … }` LITERAL — the SHARED constructor both engines
/// call. It builds an `OrderedMap` in source/insertion order; a non-hashable key
/// faults through `cmp_guard` (identically on both engines), and a DUPLICATE key
/// (runtime value) faults `FAULT_MAP_DUP_KEY` (TPZ4601) at the literal's span —
/// distinct from `Map.insert`'s silent overwrite. The pairs lower in source
/// order, so the first duplicate encountered is the one reported.
pub fn builtin_map_of(pairs: Vec<(Value, Value)>, span: Span) -> Result<Value, RtError> {
    let mut map = OrderedMap::new();
    for (key, value) in pairs {
        let canon = canonical_key(&key).map_err(|e| cmp_guard(e, span))?;
        if !map.try_insert(canon, value) {
            return Err(fault(
                codes::FAULT_MAP_DUP_KEY,
                "duplicate key in `map { … }` literal",
                span,
            ));
        }
    }
    Ok(Value::Map(Rc::new(RefCell::new(map))))
}
