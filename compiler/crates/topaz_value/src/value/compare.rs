use super::*;

/// Why two values could not be compared (a TPZ5xxx dynamic guard at
/// the operator site; valid checked programs never reach it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmpError {
    /// A participant is not a comparable type (§2): functions,
    /// `File`, templates, `Map`/`Set`.
    NotComparable(&'static str),
    /// Records with incompatible field sets (§2: static type error).
    RecordShape,
    /// Structural fuel exhausted (cyclic value in an unchecked
    /// program).
    Fuel,
}

/// §2 equality. Different comparable runtime kinds compare unequal
/// (union-equality semantics: the actual runtime values are of
/// different member types); non-comparable participants are guard
/// errors, never `false`.
pub fn values_equal(a: &Value, b: &Value) -> Result<bool, CmpError> {
    let mut fuel = STRUCT_FUEL;
    eq(a, b, &mut fuel, 0)
}

pub(super) fn eq(a: &Value, b: &Value, fuel: &mut usize, depth: usize) -> Result<bool, CmpError> {
    use Value as V;
    if *fuel == 0 || depth > STRUCT_DEPTH {
        return Err(CmpError::Fuel);
    }
    *fuel -= 1;
    match (a, b) {
        (V::Map(_), _) | (_, V::Map(_)) => Err(CmpError::NotComparable("Map")),
        (V::Set(_), _) | (_, V::Set(_)) => Err(CmpError::NotComparable("Set")),
        (V::Resource(_), _) | (_, V::Resource(_)) => Err(CmpError::NotComparable("File")),
        (V::Regex(_), _) | (_, V::Regex(_)) => Err(CmpError::NotComparable("Regex")),
        (V::Toml(_), _) | (_, V::Toml(_)) => Err(CmpError::NotComparable("TOMLValue")),
        (V::Closure(_), _) | (_, V::Closure(_)) => Err(CmpError::NotComparable("function")),
        (V::Builtin { .. }, _) | (_, V::Builtin { .. }) => Err(CmpError::NotComparable("function")),
        (V::Composed(_), _) | (_, V::Composed(_)) => Err(CmpError::NotComparable("function")),
        (V::Range { .. }, _) | (_, V::Range { .. }) => Err(CmpError::NotComparable("range")),
        // §22 JSONValue is non-comparable (the checker rejects `==`); equality is a
        // Deferred because value-versus-lexeme number identity remains unsettled,
        // so the runtime GUARDS like Map/Set/File rather than pre-committing.
        (V::Json(_), _) | (_, V::Json(_)) => Err(CmpError::NotComparable("JSONValue")),
        (V::ByteBuffer(_), _) | (_, V::ByteBuffer(_)) => Err(CmpError::NotComparable("ByteBuffer")),
        // `Template` and `Namespace` are intentionally NOT guarded
        // here: they fall through to `Ok(false)` below, matching the
        // interpreter exactly (§2 non-comparability of these is
        // enforced statically by the checker, so a runtime comparison
        // is unreachable in checked programs; behavior-neutrality of
        // the E-1c migration requires preserving the existing
        // unchecked-path result rather than tightening it here).
        (V::Int(x), V::Int(y)) => Ok(x == y),
        // IEEE-754: NaN != NaN (§2).
        (V::Float(x), V::Float(y)) => Ok(x == y),
        (V::Str(x), V::Str(y)) => Ok(x == y),
        // §8 (v5.4) `Bytes` equality is BYTE-WISE: equal iff same length + same
        // bytes (a scalar-like leaf, like `string` — NOT guarded). A `Bytes` vs a
        // non-`Bytes` falls through to `Ok(false)` (distinct union-member values),
        // never a fault (the checker keeps `==` within one comparable kind).
        (V::Bytes(x), V::Bytes(y)) => Ok(x == y),
        (V::Path(x), V::Path(y)) => Ok(x == y),
        (V::RegexMatch(x), V::RegexMatch(y)) => Ok(x == y),
        (V::Url(x), V::Url(y)) => Ok(x.canonical == y.canonical),
        (V::Date(x), V::Date(y)) => Ok(x == y),
        (V::BigInt(x), V::BigInt(y)) => Ok(x == y),
        (V::Decimal(x), V::Decimal(y)) => Ok(x == y),
        (V::Bool(x), V::Bool(y)) => Ok(x == y),
        (V::Unit, V::Unit) | (V::Null, V::Null) | (V::None, V::None) => Ok(true),
        (V::Some(x), V::Some(y)) | (V::Ok(x), V::Ok(y)) | (V::Err(x), V::Err(y)) => {
            eq(x, y, fuel, depth + 1)
        }
        // §3 enums compare NOMINALLY: same enum_id + same variant + recursively
        // equal payloads (position-wise). A different enum_id / variant / arity is
        // unequal (not a fault). The recursion is fuel/depth bounded (a payload may
        // hold a mutable array/map an unchecked program can make cyclic).
        (
            V::Enum {
                enum_id: ea,
                declaration_identity: da,
                variant: va,
                payloads: pa,
                ..
            },
            V::Enum {
                enum_id: eb,
                declaration_identity: db,
                variant: vb,
                payloads: pb,
                ..
            },
        ) => {
            if nominal_declaration_identity(ea, da.as_deref())
                != nominal_declaration_identity(eb, db.as_deref())
                || va != vb
                || pa.len() != pb.len()
            {
                Ok(false)
            } else {
                for (x, y) in pa.iter().zip(pb.iter()) {
                    if !eq(x, y, fuel, depth + 1)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
        // §3 nominal records compare NOMINALLY: same record_id ⇒ same field set
        // and ORDER (decl-order), so the fields compare position-wise. A different
        // record_id is unequal (not a fault). Fuel/depth bounded (a field may hold
        // a mutable array/map an unchecked program can make cyclic).
        (
            V::NominalRecord {
                record_id: ra,
                declaration_identity: da,
                fields: fa,
                ..
            },
            V::NominalRecord {
                record_id: rb,
                declaration_identity: db,
                fields: fb,
                ..
            },
        ) => {
            if nominal_declaration_identity(ra, da.as_deref())
                != nominal_declaration_identity(rb, db.as_deref())
                || fa.len() != fb.len()
            {
                Ok(false)
            } else {
                for ((_, x), (_, y)) in fa.iter().zip(fb.iter()) {
                    if !eq(x, y, fuel, depth + 1)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
        // §3 newtypes compare NOMINALLY: same newtype_id ⇒ compare the wrapped
        // inner values (so `==` consults the BASE type's comparability — a newtype
        // over a Map/function inner faults like the base does). A different
        // newtype_id is unequal (not a fault). Fuel/depth bounded.
        (
            V::Newtype {
                newtype_id: na,
                declaration_identity: da,
                inner: ia,
                ..
            },
            V::Newtype {
                newtype_id: nb,
                declaration_identity: db,
                inner: ib,
                ..
            },
        ) => {
            if nominal_declaration_identity(na, da.as_deref())
                != nominal_declaration_identity(nb, db.as_deref())
            {
                Ok(false)
            } else {
                eq(ia, ib, fuel, depth + 1)
            }
        }
        (V::Array(x), V::Array(y)) => {
            // No pointer fast path: nested non-comparable values must
            // still raise the guard, and cyclic self-comparison fuels
            // out rather than claiming reflexive equality.
            let (x, y) = (x.borrow(), y.borrow());
            if x.len() != y.len() {
                return Ok(false);
            }
            for (xv, yv) in x.iter().zip(y.iter()) {
                if !eq(xv, yv, fuel, depth + 1)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (V::Record(x), V::Record(y)) => {
            // §2: field-wise by name; incompatible field sets are a
            // static type error → dynamic guard here.
            if x.len() != y.len() || !x.keys().eq(y.keys()) {
                return Err(CmpError::RecordShape);
            }
            for (xf, yf) in x.values().zip(y.values()) {
                if !eq(xf, yf, fuel, depth + 1)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        // Different comparable kinds: distinct union-member values.
        _ => Ok(false),
    }
}

/// §2 ORDERING (`<`/`<=`/`>`/`>=` and `sorted`/`sortedBy`): the SHARED total-order
/// leaf both engines call, so an ordering decision cannot drift between interp and
/// boxed emit (run≡build by construction). Order-comparable scalars are
/// `int`/`float`/`string`; the NOMINAL kinds order STRUCTURALLY, consistent with
/// `==` (no `derives(Order)` gate — derive applies to generic bounds, not equality):
/// a nominal record by its fields in DECLARATION order (lexicographic), an enum by
/// variant INDEX then payloads left-to-right, a newtype by its base. A
/// non-order-comparable participant (a Map/Set/function inner, two distinct
/// runtime kinds, a `bool`/`unit`/`null`/Option/Result, a different nominal id)
/// raises the SAME `CmpError` family `eq` would — the checker rejects these
/// statically, so a valid checked program never reaches the guard. RECURSIVE and
/// fuel/depth bounded (an unchecked program can make a field/payload cyclic).
pub fn values_compare(a: &Value, b: &Value) -> Result<std::cmp::Ordering, CmpError> {
    let mut fuel = STRUCT_FUEL;
    compare(a, b, &mut fuel, 0)
}

pub(super) fn compare(
    a: &Value,
    b: &Value,
    fuel: &mut usize,
    depth: usize,
) -> Result<std::cmp::Ordering, CmpError> {
    use Value as V;
    use std::cmp::Ordering;
    if *fuel == 0 || depth > STRUCT_DEPTH {
        return Err(CmpError::Fuel);
    }
    *fuel -= 1;
    match (a, b) {
        (V::Int(x), V::Int(y)) => Ok(x.cmp(y)),
        // IEEE-754 total intent: ordering excludes NaN at the checker (floats ARE
        // orderable, NaN is a value-level concern), so `partial_cmp` is `Some` for
        // every reachable pair; a NaN (only via an unchecked program) is treated as
        // Equal rather than panicking — total and deterministic.
        (V::Float(x), V::Float(y)) => Ok(x.partial_cmp(y).unwrap_or(Ordering::Equal)),
        (V::Str(x), V::Str(y)) => Ok(x.cmp(y)),
        // §8 (v5.4) `Bytes` order LEXICOGRAPHICALLY by byte value (`[u8]::cmp`,
        // like `string`): a prefix sorts before its extension, byte 0x00 < 0xff.
        // Order-comparable (the checker accepts `<`/`sorted` on `Bytes`), so this
        // is reached for valid programs — total + deterministic, the same leaf on
        // both engines.
        (V::Bytes(x), V::Bytes(y)) => Ok(x.cmp(y)),
        (V::Path(x), V::Path(y)) => Ok(x.cmp(y)),
        (V::Url(x), V::Url(y)) => Ok(x.canonical.as_ref().cmp(y.canonical.as_ref())),
        (V::Date(x), V::Date(y)) => Ok(x.cmp(y)),
        (V::BigInt(x), V::BigInt(y)) => Ok(x.cmp(y)),
        (V::Decimal(x), V::Decimal(y)) => Ok(x.cmp(y)),
        // §3 enums order NOMINALLY: same enum_id ⇒ by variant INDEX, then payloads
        // left-to-right (the first unequal payload decides). A different enum_id is a
        // guard (the checker rejects cross-enum `<`); a differing arity is unreachable
        // for a same-variant pair, but bounded by the zip for an unchecked program.
        (
            V::Enum {
                enum_id: ea,
                declaration_identity: da,
                variant_index: ia,
                payloads: pa,
                ..
            },
            V::Enum {
                enum_id: eb,
                declaration_identity: db,
                variant_index: ib,
                payloads: pb,
                ..
            },
        ) => {
            if ea.as_ref() == "RoundingMode" || eb.as_ref() == "RoundingMode" {
                return Err(CmpError::NotComparable("RoundingMode"));
            }
            if nominal_declaration_identity(ea, da.as_deref())
                != nominal_declaration_identity(eb, db.as_deref())
            {
                return Err(CmpError::NotComparable("enum"));
            }
            // DECLARATION-ORDER: order by the variant's decl index (NOT its name),
            // then by payloads left-to-right within the same variant (§4).
            match ia.cmp(ib) {
                Ordering::Equal => {
                    for (x, y) in pa.iter().zip(pb.iter()) {
                        match compare(x, y, fuel, depth + 1)? {
                            Ordering::Equal => continue,
                            ord => return Ok(ord),
                        }
                    }
                    Ok(pa.len().cmp(&pb.len()))
                }
                ord => Ok(ord),
            }
        }
        // §3 nominal records order NOMINALLY: same record_id ⇒ field-wise in DECL
        // order (the value carries decl-ordered fields), the first unequal field
        // decides — lexicographic. A different record_id is a guard.
        (
            V::NominalRecord {
                record_id: ra,
                declaration_identity: da,
                fields: fa,
                ..
            },
            V::NominalRecord {
                record_id: rb,
                declaration_identity: db,
                fields: fb,
                ..
            },
        ) => {
            if nominal_declaration_identity(ra, da.as_deref())
                != nominal_declaration_identity(rb, db.as_deref())
            {
                return Err(CmpError::NotComparable("record"));
            }
            for ((_, x), (_, y)) in fa.iter().zip(fb.iter()) {
                match compare(x, y, fuel, depth + 1)? {
                    Ordering::Equal => continue,
                    ord => return Ok(ord),
                }
            }
            Ok(fa.len().cmp(&fb.len()))
        }
        // §3 newtypes order by their BASE: same newtype_id ⇒ compare the wrapped
        // inner (so a newtype over a non-orderable base guards like the base). A
        // different newtype_id is a guard.
        (
            V::Newtype {
                newtype_id: na,
                declaration_identity: da,
                inner: ia,
                ..
            },
            V::Newtype {
                newtype_id: nb,
                declaration_identity: db,
                inner: ib,
                ..
            },
        ) => {
            if nominal_declaration_identity(na, da.as_deref())
                != nominal_declaration_identity(nb, db.as_deref())
            {
                return Err(CmpError::NotComparable("newtype"));
            }
            compare(ia, ib, fuel, depth + 1)
        }
        // Everything else is NOT order-comparable: the checker rejects `<` and a
        // non-orderable `.sorted()`/`.sortedBy` key statically, so this is reachable
        // only in an unchecked program or via a leaf the checker missed. Name the
        // kind for a clear guard (mirrors `eq`'s `NotComparable`).
        _ => Err(CmpError::NotComparable(a.kind())),
    }
}

/// STABLE in-place ascending sort of a `Vec<Value>` by a FALLIBLE comparator (the
/// shared `values_compare` leaf, possibly via a key projection). `sort_by` cannot
/// thread a `Result`, so the comparator's FIRST `CmpError` is captured and, if any,
/// raised as a single GUARD_COMPARE fault at `span` (via `cmp_guard`); on error the
/// comparator returns `Equal` to keep the sort total, but the result is discarded.
/// `sort_by` is Rust's STABLE merge sort, so equal elements/keys keep input order —
/// the run≡build stability guarantee for both `sorted` and `sortedBy`.
pub(super) fn sort_values_stable(
    out: &mut [Value],
    span: Span,
    mut cmp: impl FnMut(&Value, &Value) -> Result<std::cmp::Ordering, CmpError>,
) -> Result<(), RtError> {
    let mut err: Option<CmpError> = None;
    out.sort_by(|a, b| match cmp(a, b) {
        Ok(ord) => ord,
        Err(e) => {
            if err.is_none() {
                err = Some(e);
            }
            std::cmp::Ordering::Equal
        }
    });
    if let Some(e) = err {
        return Err(cmp_guard(e, span));
    }
    Ok(())
}

/// §22 (v5.4) `sortedBy` core: STABLY sort `items` by the parallel `keys` (the
/// callback projections `f(item)`, already collected by each engine in element
/// order), returning a NEW element vector. The keys order through the SHARED
/// `values_compare` leaf, so the projection sort is byte-identical run≡build; a
/// non-order-comparable KEY (reachable only `--unchecked` — the checker rejects a
/// non-orderable key type) faults GUARD_COMPARE at `span` via the SAME mapping `<`
/// and `sorted` use. STABLE (Rust's merge sort over an index permutation), so equal
/// keys preserve input element order. `items.len() == keys.len()` (the caller pairs
/// them); a mismatch is a caller bug (the index sort just ignores the surplus).
pub fn sorted_by_keys(items: &[Value], keys: &[Value], span: Span) -> Result<Vec<Value>, RtError> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    let mut err: Option<CmpError> = None;
    // Sort an INDEX permutation (not the keys/items themselves) so the sort stays
    // stable over the ORIGINAL positions and the items move with their keys.
    order.sort_by(|&i, &j| match values_compare(&keys[i], &keys[j]) {
        Ok(ord) => ord,
        Err(e) => {
            if err.is_none() {
                err = Some(e);
            }
            std::cmp::Ordering::Equal
        }
    });
    if let Some(e) = err {
        return Err(cmp_guard(e, span));
    }
    Ok(order.into_iter().map(|i| items[i].clone()).collect())
}
