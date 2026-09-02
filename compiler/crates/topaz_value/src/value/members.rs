use super::*;

pub(super) fn bound_member(object: &Value, kind: Builtin) -> Value {
    Value::Builtin {
        kind,
        recv: Some(Rc::new(object.clone())),
    }
}

/// The execution family of a receiver builtin. `Method` enters the synchronous
/// [`call_method`] leaf, `Callback` needs an engine continuation, and `Resource`
/// crosses the host boundary. Keeping this beside the `(receiver, member)`
/// catalog lets member lookup, interpreter binding, generated preflight, and
/// named-argument binding consume one runtime identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverBuiltinRoute {
    Method,
    Callback,
    Resource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverBuiltin {
    pub name: &'static str,
    pub kind: Builtin,
    pub route: ReceiverBuiltinRoute,
    pub mutates: bool,
}

/// Receiver-independent facts for a source member spelling. Overloaded builtin
/// receiver names share their execution route and mutability (`get` is always a
/// read-only method, `insert` is always a mutator, and `map` is always callback
/// driven), while the receiver-specific [`Builtin`] tag remains in
/// [`ReceiverBuiltin`]. The emitter consumes this projection before a runtime
/// receiver value exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverBuiltinNameShape {
    pub route: ReceiverBuiltinRoute,
    pub mutates: bool,
}

impl ReceiverBuiltin {
    const fn method(name: &'static str, kind: Builtin, mutates: bool) -> Self {
        Self {
            name,
            kind,
            route: ReceiverBuiltinRoute::Method,
            mutates,
        }
    }

    const fn callback(name: &'static str, kind: Builtin, mutates: bool) -> Self {
        Self {
            name,
            kind,
            route: ReceiverBuiltinRoute::Callback,
            mutates,
        }
    }

    const fn resource(name: &'static str, kind: Builtin) -> Self {
        Self {
            name,
            kind,
            route: ReceiverBuiltinRoute::Resource,
            mutates: false,
        }
    }
}

use Builtin::*;
use ReceiverBuiltin as R;

pub(super) const ARRAY_RECEIVER_BUILTINS: &[R] = &[
    R::method("push", ArrPush, true),
    R::method("pop", ArrPop, true),
    R::method("clear", ArrClear, true),
    R::method("reverse", ArrReverse, true),
    R::method("insert", ArrInsert, true),
    R::method("removeAt", ArrRemoveAt, true),
    R::method("sort", ArrSort, true),
    R::callback("sortBy", ArrSortBy, true),
    R::callback("retain", ArrRetain, true),
    R::method("get", ArrGet, false),
    R::method("slice", ArrSlice, false),
    R::method("join", ArrJoin, false),
    R::method("indexOf", ArrIndexOf, false),
    R::method("sorted", ArrSorted, false),
    R::callback("sortedBy", ArrSortedBy, false),
    R::callback("filter", FilterFn, false),
    R::callback("map", MapFn, false),
    R::callback("reduce", ReduceFn, false),
];
pub(super) const MAP_RECEIVER_BUILTINS: &[R] = &[
    R::method("insert", MapInsert, true),
    R::method("get", MapGet, false),
    R::method("getOr", MapGetOr, false),
    R::method("remove", MapRemove, true),
    R::method("containsKey", MapContainsKey, false),
    R::method("isEmpty", MapIsEmpty, false),
    R::method("clear", MapClear, true),
    R::callback("update", MapUpdate, true),
    R::callback("mapValues", MapMapValues, false),
    R::callback("filter", MapFilter, false),
];
pub(super) const SET_RECEIVER_BUILTINS: &[R] = &[
    R::method("add", SetAdd, true),
    R::method("remove", SetRemove, true),
    R::method("contains", SetContains, false),
    R::method("isEmpty", SetIsEmpty, false),
    R::method("toArray", SetToArray, false),
    R::method("union", SetUnion, false),
    R::method("intersection", SetIntersection, false),
    R::method("difference", SetDifference, false),
    R::method("clear", SetClear, true),
];
pub(super) const STRING_RECEIVER_BUILTINS: &[R] = &[
    R::method("scalars", Scalars, false),
    R::method("startsWith", StrStartsWith, false),
    R::method("endsWith", StrEndsWith, false),
    R::method("contains", StrContains, false),
    R::method("indexOf", StrIndexOf, false),
    R::method("lastIndexOf", StrLastIndexOf, false),
    R::method("codePointAt", StrCodePointAt, false),
    R::method("trim", StrTrim, false),
    R::method("trimStart", StrTrimStart, false),
    R::method("trimEnd", StrTrimEnd, false),
    R::method("split", StrSplit, false),
    R::method("byteLength", StrByteLength, false),
    R::method("slice", StrSlice, false),
    R::method("replace", StrReplace, false),
];
pub(super) const INT_RECEIVER_BUILTINS: &[R] = &[
    R::method("atLeast", IntAtLeast, false),
    R::method("atMost", IntAtMost, false),
];
pub(super) const OPTION_RECEIVER_BUILTINS: &[R] = &[
    R::method("okOr", OkOr, false),
    R::callback("okOrElse", OkOrElse, false),
    R::callback("map", OptionMap, false),
    R::callback("flatMap", OptionFlatMap, false),
];
pub(super) const RESULT_RECEIVER_BUILTINS: &[R] = &[
    R::callback("map", ResultMap, false),
    R::callback("flatMap", ResultFlatMap, false),
];
pub(super) const RESOURCE_RECEIVER_BUILTINS: &[R] = &[
    R::resource("read", FileRead),
    R::resource("write", FileWrite),
    R::resource("close", FileClose),
];
pub(super) const JSON_RECEIVER_BUILTINS: &[R] = &[
    R::method("kind", JsonKind, false),
    R::method("isNull", JsonIsNull, false),
    R::method("asString", JsonAsString, false),
    R::method("asBool", JsonAsBool, false),
    R::method("asInt", JsonAsInt, false),
    R::method("numberText", JsonNumberText, false),
    R::method("get", JsonGet, false),
    R::method("at", JsonAt, false),
    R::method("length", JsonLength, false),
    R::method("asArray", JsonAsArray, false),
    R::method("keys", JsonKeys, false),
    R::method("values", JsonValues, false),
];
pub(super) const NEWTYPE_RECEIVER_BUILTINS: &[R] = &[R::method("value", NewtypeValue, false)];
pub(super) const BYTES_RECEIVER_BUILTINS: &[R] = &[
    R::method("decodeUtf8", BytesDecodeUtf8, false),
    R::method("toHex", BytesToHex, false),
    R::method("toBase64", BytesToBase64, false),
    R::method("length", BytesLength, false),
    R::method("isEmpty", BytesIsEmpty, false),
    R::method("get", BytesGet, false),
    R::method("slice", BytesSlice, false),
    R::method("toArray", BytesToArray, false),
];
pub(super) const BYTE_BUFFER_RECEIVER_BUILTINS: &[R] = &[
    R::method("length", ByteBufferLength, false),
    R::method("get", ByteBufferGet, false),
    R::method("set", ByteBufferSet, true),
    R::method("fill", ByteBufferFill, true),
    R::method("copy", ByteBufferCopy, true),
    R::method("toBytes", ByteBufferToBytes, false),
];
pub(super) const PATH_RECEIVER_BUILTINS: &[R] = &[
    R::method("join", PathJoin, false),
    R::method("parent", PathParent, false),
    R::method("fileName", PathFileName, false),
    R::method("extension", PathExtension, false),
    R::method("withExtension", PathWithExtension, false),
    R::method("normalize", PathNormalize, false),
    R::method("toString", PathToString, false),
];
pub(super) const REGEX_RECEIVER_BUILTINS: &[R] = &[
    R::method("isMatch", RegexIsMatch, false),
    R::method("find", RegexFind, false),
    R::method("findAll", RegexFindAll, false),
    R::method("split", RegexSplit, false),
    R::method("replaceAll", RegexReplaceAll, false),
];
pub(super) const URL_RECEIVER_BUILTINS: &[R] = &[
    R::method("scheme", UrlScheme, false),
    R::method("host", UrlHost, false),
    R::method("path", UrlPath, false),
    R::method("query", UrlQuery, false),
    R::method("fragment", UrlFragment, false),
    R::method("toString", UrlToString, false),
];
pub(super) const DATE_RECEIVER_BUILTINS: &[R] = &[
    R::method("toIso", DateToIso, false),
    R::method("addDays", DateAddDays, false),
    R::method("year", DateYear, false),
    R::method("month", DateMonth, false),
    R::method("day", DateDay, false),
];
pub(super) const BIGINT_RECEIVER_BUILTINS: &[R] = &[
    R::method("toString", BigIntToString, false),
    R::method("toInt", BigIntToInt, false),
    R::method("div", BigIntDiv, false),
    R::method("mod", BigIntMod, false),
];
pub(super) const DECIMAL_RECEIVER_BUILTINS: &[R] = &[
    R::method("toString", DecimalToString, false),
    R::method("scale", DecimalScale, false),
    R::method("toInt", DecimalToInt, false),
    R::method("round", DecimalRound, false),
    R::method("div", DecimalDiv, false),
];

pub(super) const RECEIVER_BUILTIN_CATALOGS: &[&[R]] = &[
    ARRAY_RECEIVER_BUILTINS,
    MAP_RECEIVER_BUILTINS,
    SET_RECEIVER_BUILTINS,
    STRING_RECEIVER_BUILTINS,
    INT_RECEIVER_BUILTINS,
    OPTION_RECEIVER_BUILTINS,
    RESULT_RECEIVER_BUILTINS,
    RESOURCE_RECEIVER_BUILTINS,
    JSON_RECEIVER_BUILTINS,
    NEWTYPE_RECEIVER_BUILTINS,
    BYTES_RECEIVER_BUILTINS,
    BYTE_BUFFER_RECEIVER_BUILTINS,
    PATH_RECEIVER_BUILTINS,
    REGEX_RECEIVER_BUILTINS,
    URL_RECEIVER_BUILTINS,
    DATE_RECEIVER_BUILTINS,
    BIGINT_RECEIVER_BUILTINS,
    DECIMAL_RECEIVER_BUILTINS,
];

pub(super) fn receiver_builtin_catalog(object: &Value) -> &'static [R] {
    match object {
        Value::Array(_) => ARRAY_RECEIVER_BUILTINS,
        Value::Map(_) => MAP_RECEIVER_BUILTINS,
        Value::Set(_) => SET_RECEIVER_BUILTINS,
        Value::Str(_) => STRING_RECEIVER_BUILTINS,
        Value::Int(_) => INT_RECEIVER_BUILTINS,
        Value::Some(_) | Value::None => OPTION_RECEIVER_BUILTINS,
        Value::Ok(_) | Value::Err(_) => RESULT_RECEIVER_BUILTINS,
        Value::Resource(_) => RESOURCE_RECEIVER_BUILTINS,
        Value::Json(_) => JSON_RECEIVER_BUILTINS,
        Value::Newtype { .. } => NEWTYPE_RECEIVER_BUILTINS,
        Value::Bytes(_) => BYTES_RECEIVER_BUILTINS,
        Value::ByteBuffer(_) => BYTE_BUFFER_RECEIVER_BUILTINS,
        Value::Path(_) => PATH_RECEIVER_BUILTINS,
        Value::Regex(_) => REGEX_RECEIVER_BUILTINS,
        Value::Url(_) => URL_RECEIVER_BUILTINS,
        Value::Date(_) => DATE_RECEIVER_BUILTINS,
        Value::BigInt(_) => BIGINT_RECEIVER_BUILTINS,
        Value::Decimal(_) => DECIMAL_RECEIVER_BUILTINS,
        _ => &[],
    }
}

/// Canonical runtime identity for every builtin receiver member. This is the
/// single owner of receiver kind + source member name → builtin tag, including
/// whether obtaining that member requires a mutable root and which execution
/// family owns the call.
pub fn receiver_builtin(object: &Value, field: &str) -> Option<R> {
    receiver_builtin_catalog(object)
        .iter()
        .copied()
        .find(|receiver| receiver.name == field)
}

/// Reverse lookup used after the interpreter has obtained a receiver-bound
/// builtin value. The source member spelling and execution route remain owned
/// by the same per-receiver catalog rather than repeated in engine dispatch.
pub fn receiver_builtin_by_kind(object: &Value, kind: Builtin) -> Option<R> {
    receiver_builtin_catalog(object)
        .iter()
        .copied()
        .find(|receiver| receiver.kind == kind)
}

/// Project a source member spelling before its concrete receiver is available.
/// Every overload must agree on route and mutability; a future conflicting
/// overload stays unclassified instead of letting an emitter guess which
/// runtime receiver will arrive.
pub fn receiver_builtin_name_shape(name: &str) -> Option<ReceiverBuiltinNameShape> {
    let mut shape = None;
    for receiver in RECEIVER_BUILTIN_CATALOGS
        .iter()
        .flat_map(|catalog| catalog.iter())
        .filter(|receiver| receiver.name == name)
    {
        let current = ReceiverBuiltinNameShape {
            route: receiver.route,
            mutates: receiver.mutates,
        };
        match shape {
            None => shape = Some(current),
            Some(existing) if existing == current => {}
            Some(_) => return None,
        }
    }
    shape
}

/// §8/§22.2 member access — the `obj.field` cases that resolve to a value
/// before the caller's fallback: record fields, access-only properties
/// (`.length`, `.keys`, etc.), and read-only receiver methods that can be
/// represented as a receiver-bound [`Builtin`]. Mutators and callback-driven
/// HOF methods stay out of this leaf because they need a `mut` root or an
/// engine continuation. Shared so property values, string-`.length` faults, and
/// first-class read-only bound methods cannot drift between engines.
pub fn member_value(object: &Value, field: &str, span: Span) -> Result<Option<Value>, RtError> {
    match (object, field) {
        (Value::Record(map), _) => Ok(Some(map.get(field).cloned().ok_or_else(|| {
            fault(
                codes::GUARD_NO_FIELD,
                format!("record has no field `{field}`"),
                span,
            )
        })?)),
        // §3 a NOMINAL record's field access (`user.name`) — a linear lookup over
        // the declaration-ordered fields. Shared so interp + boxed emit read the
        // same field value and raise the identical no-field fault (run≡build).
        (
            Value::NominalRecord {
                record_id, fields, ..
            },
            _,
        ) => Ok(Some(
            fields
                .iter()
                .find(|(n, _)| n.as_ref() == field)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| {
                    fault(
                        codes::GUARD_NO_FIELD,
                        format!("record `{record_id}` has no field `{field}`"),
                        span,
                    )
                })?,
        )),
        (Value::Array(items), "length") => Ok(Some(Value::Int(items.borrow().len() as i64))),
        (Value::Map(map), "keys") => Ok(Some(Value::array(map.borrow().keys()))),
        (Value::Map(map), "values") => Ok(Some(Value::array(map.borrow().values()))),
        (Value::Map(map), "length") => Ok(Some(Value::Int(map.borrow().len() as i64))),
        (Value::Set(set), "length") => Ok(Some(Value::Int(set.borrow().len() as i64))),
        // §22 `m.entries` — (key, value) pairs as records `{ key, value }`, in
        // insertion order (parallels `keys`/`values`). A fresh snapshot.
        (Value::Map(map), "entries") => Ok(Some(Value::array(
            map.borrow()
                .pairs()
                .into_iter()
                .map(|(k, v)| Value::record([("key".to_string(), k), ("value".to_string(), v)]))
                .collect(),
        ))),
        (Value::Str(_), "length") => Err(fault(
            codes::GUARD_TYPE,
            "strings expose no `.length`; use `s.scalars().length` (§1)",
            span,
        )),
        // §16 template accessors: the producing tag, and the literal text segments
        // between interpolations (n+1 of them). The interpolated VALUES are NOT
        // exposed — that would defeat the sql/sh injection-safety the §16 rules
        // exist for. `parts` is materialized fresh (a template is immutable).
        (Value::Template(t), "tag") => Ok(t
            .as_any()
            .downcast_ref::<TemplateData>()
            .map(|d| Value::str(&d.tag))),
        (Value::Template(t), "parts") => Ok(t
            .as_any()
            .downcast_ref::<TemplateData>()
            .map(|d| Value::array(d.parts.iter().map(Value::str).collect()))),
        (Value::RegexMatch(m), "start") => Ok(Some(Value::Int(m.start))),
        (Value::RegexMatch(m), "end") => Ok(Some(Value::Int(m.end))),
        (Value::RegexMatch(m), "text") => Ok(Some(Value::Str(m.text.clone()))),
        (Value::RegexMatch(m), "groups") => Ok(Some(regex_match_groups_value(m))),
        (Value::RegexMatch(m), "named") => Ok(Some(regex_match_named_value(m))),
        // Synchronous, non-mutating receiver methods are first-class values.
        // Callback, resource, and mutating members stay engine-routed because
        // they need a continuation, host, or mutable-root proof respectively.
        _ => Ok(receiver_builtin(object, field)
            .filter(|builtin| builtin.route == ReceiverBuiltinRoute::Method && !builtin.mutates)
            .map(|builtin| bound_member(object, builtin.kind))),
    }
}

/// The fault for `obj.field` where `field` is not a member of `obj` — the
/// shared form so the interpreter's fall-through arm and the emitter's
/// [`member_value_required`] raise the identical fault.
pub fn no_member_fault(object: &Value, field: &str, span: Span) -> RtError {
    fault(
        codes::GUARD_NO_FIELD,
        format!("`{}` has no member named `{field}`", object.kind()),
        span,
    )
}

/// A pure member access that MUST yield a value: [`member_value`] with a
/// `None` (no such pure member) turned into [`no_member_fault`]. The emitter
/// uses this for an `obj.field` whose `field` is not a bound-method name, so
/// a `None` means a genuinely absent member — exactly the point at which the
/// interpreter's method arms also miss and fault.
pub fn member_value_required(object: &Value, field: &str, span: Span) -> Result<Value, RtError> {
    member_value(object, field, span)?.ok_or_else(|| no_member_fault(object, field, span))
}

/// Bind a receiver builtin after [`member_value`] has missed. Keeping the
/// receiver/type lookup and the resulting [`Value::Builtin`] construction in
/// one leaf lets every engine represent synchronous, callback-driven, resource,
/// and mutating receiver members with the same runtime identity. The caller
/// still owns mutable-root admission because that depends on source bindings,
/// not on the receiver value itself.
pub fn bind_receiver_builtin(object: Value, field: &str, span: Span) -> Result<Value, RtError> {
    let receiver =
        receiver_builtin(&object, field).ok_or_else(|| no_member_fault(&object, field, span))?;
    Ok(Value::Builtin {
        kind: receiver.kind,
        recv: Some(Rc::new(object)),
    })
}

/// §22.2 a bound-method CALL `recv.method(args)` on a builtin receiver — the
/// receiver-typed methods both engines dispatch identically: the READ-ONLY ones
/// (an array's `.get(i)`, a map's `.get(k)`, a string's `.scalars()`) AND the
/// IN-PLACE MUTATORS (an array's `.push`, a map's `.insert`/`.remove`, a set's
/// `.add`/`.remove`), which mutate through the receiver's shared `Rc<RefCell>`
/// cell so the change reaches the binding. Shared so the emitter's bound-method
/// call and the interpreter's `call_builtin` (whose `ArrGet`/`MapGet`/`Scalars`
/// read arms and `ArrPush`/`MapInsert`/`MapRemove`/`SetAdd`/`SetRemove` mutate
/// arms all call THIS) cannot drift. The receiver type is resolved at runtime —
/// the emitter cannot know it, so it emits this after a [`member_value`] miss (a
/// record field of the method's name takes precedence and is called as a closure
/// instead, matching `member_access`).
///
/// TWO spans, because the interpreter raises these faults at two different
/// points: a receiver with NO such method faults at `member_span` (the
/// interpreter's `member_access` faults at the member expression, BEFORE the
/// call is scheduled); an arity or argument-type fault uses `call_span` (the
/// interpreter's `call_builtin` threads the call's span). Mutating an IMMUTABLE
/// receiver is refused by the emitter's separate static `mut`-root gate, not
/// here. The RESOURCE methods (`file.read`/`write`/`close`) are deliberately
/// absent — they need the host, so both engines route them through the separate
/// `call_resource_method` leaf instead.
pub fn call_method(
    recv: Value,
    method: &str,
    args: Vec<Value>,
    member_span: Span,
    call_span: Span,
) -> Result<Value, RtError> {
    Ok(match (&recv, method) {
        // §3 (v5.4) `id.value()` — unwrap the newtype to its base value. Zero-arg.
        // `member_access`/emit only bind this on a `Value::Newtype` receiver, so the
        // arm is total; the defensive `newtype_value` leaf (with the recv's own id)
        // keeps the unwrap + `--unchecked` fault byte-identical run≡build.
        (
            Value::Newtype {
                newtype_id,
                declaration_identity,
                ..
            },
            "value",
        ) => {
            let [] = exact_args(args, call_span)?;
            let id = newtype_id.clone();
            let identity = declaration_identity.clone();
            return newtype_value_with_identity(recv, &id, identity.as_deref(), call_span);
        }
        // §22 JSONValue accessors — read-only inspection of a parsed JSON tree. The
        // `as*`/`get`/`at`/`length` accessors return `Option`: `None` on a type/shape
        // mismatch (a non-string `get` key / non-int `at` index still faults, like
        // the collection accessors). All zero-arg except `get`/`at`.
        (Value::Json(node), "kind") => {
            let [] = exact_args(args, call_span)?;
            Value::str(json_kind_name(node))
        }
        (Value::Json(node), "isNull") => {
            let [] = exact_args(args, call_span)?;
            Value::Bool(matches!(&**node, JsonValue::Null))
        }
        (Value::Json(node), "asString") => {
            let [] = exact_args(args, call_span)?;
            match &**node {
                JsonValue::String(s) => Value::Some(Rc::new(Value::Str(s.clone()))),
                _ => Value::None,
            }
        }
        (Value::Json(node), "asBool") => {
            let [] = exact_args(args, call_span)?;
            match &**node {
                JsonValue::Bool(b) => Value::Some(Rc::new(Value::Bool(*b))),
                _ => Value::None,
            }
        }
        (Value::Json(node), "asInt") => {
            let [] = exact_args(args, call_span)?;
            match &**node {
                JsonValue::Number(n) => match n.int {
                    Some(i) => Value::Some(Rc::new(Value::Int(i))),
                    None => Value::None,
                },
                _ => Value::None,
            }
        }
        (Value::Json(node), "numberText") => {
            let [] = exact_args(args, call_span)?;
            match &**node {
                JsonValue::Number(n) => Value::Some(Rc::new(Value::Str(n.lexeme.clone()))),
                _ => Value::None,
            }
        }
        (Value::Json(node), "get") => {
            let [key] = exact_args(args, call_span)?;
            match (&**node, key) {
                (JsonValue::Object(map), Value::Str(k)) => match map.get(&*k) {
                    Some(v) => Value::Some(Rc::new(Value::Json(Rc::new(v.clone())))),
                    None => Value::None,
                },
                (JsonValue::Object(_), other) => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`JSONValue.get` takes a string key; got `{}`", other.kind()),
                        call_span,
                    ));
                }
                _ => Value::None,
            }
        }
        (Value::Json(node), "at") => {
            let [index] = exact_args(args, call_span)?;
            match (&**node, index) {
                (JsonValue::Array(items), Value::Int(i)) => {
                    if i >= 0 && (i as usize) < items.len() {
                        Value::Some(Rc::new(Value::Json(Rc::new(items[i as usize].clone()))))
                    } else {
                        Value::None
                    }
                }
                (JsonValue::Array(_), other) => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`JSONValue.at` takes an int index; got `{}`", other.kind()),
                        call_span,
                    ));
                }
                _ => Value::None,
            }
        }
        (Value::Json(node), "length") => {
            let [] = exact_args(args, call_span)?;
            match &**node {
                JsonValue::Array(items) => Value::Some(Rc::new(Value::Int(items.len() as i64))),
                JsonValue::Object(map) => Value::Some(Rc::new(Value::Int(map.len() as i64))),
                _ => Value::None,
            }
        }
        // §22 iteration accessors — a JSON array/object → a Topaz Array (of JSONValue
        // elements / string keys / JSONValue values), so a for-loop can walk it.
        (Value::Json(node), "asArray") => {
            let [] = exact_args(args, call_span)?;
            match &**node {
                JsonValue::Array(items) => Value::Some(Rc::new(Value::array(
                    items
                        .iter()
                        .map(|e| Value::Json(Rc::new(e.clone())))
                        .collect(),
                ))),
                _ => Value::None,
            }
        }
        (Value::Json(node), "keys") => {
            let [] = exact_args(args, call_span)?;
            match &**node {
                JsonValue::Object(entries) => Value::Some(Rc::new(Value::array(
                    entries.keys().map(|k| Value::Str(k.clone())).collect(),
                ))),
                _ => Value::None,
            }
        }
        (Value::Json(node), "values") => {
            let [] = exact_args(args, call_span)?;
            match &**node {
                JsonValue::Object(entries) => Value::Some(Rc::new(Value::array(
                    entries
                        .values()
                        .map(|v| Value::Json(Rc::new(v.clone())))
                        .collect(),
                ))),
                _ => Value::None,
            }
        }
        // §8 (v5.4) `Bytes` INSTANCE methods — each routes through the SAME shared
        // `builtin_bytes_*` leaf the emitter calls, so the codec is byte-identical
        // run≡build. `decodeUtf8` is fallible (`Result`); `toHex`/`toBase64`/`length`
        // are total; `slice` CLAMPS its bounds (never faults, matching `arr.slice`).
        (Value::Bytes(_), "decodeUtf8") => {
            let [] = exact_args(args, call_span)?;
            return builtin_bytes_decode_utf8(recv, call_span);
        }
        (Value::Bytes(_), "toHex") => {
            let [] = exact_args(args, call_span)?;
            return builtin_bytes_to_hex(recv, call_span);
        }
        (Value::Bytes(_), "toBase64") => {
            let [] = exact_args(args, call_span)?;
            return builtin_bytes_to_base64(recv, call_span);
        }
        (Value::Bytes(_), "length") => {
            let [] = exact_args(args, call_span)?;
            return builtin_bytes_length(recv, call_span);
        }
        (Value::Bytes(_), "isEmpty") => {
            let [] = exact_args(args, call_span)?;
            return builtin_bytes_is_empty(recv, call_span);
        }
        (Value::Bytes(_), "get") => {
            let [index] = exact_args(args, call_span)?;
            return builtin_bytes_get(recv, index, call_span);
        }
        (Value::Bytes(_), "slice") => {
            let [start, end] = exact_args(args, call_span)?;
            return builtin_bytes_slice(recv, start, end, call_span);
        }
        (Value::Bytes(_), "toArray") => {
            let [] = exact_args(args, call_span)?;
            return builtin_bytes_to_array(recv, call_span);
        }
        (Value::ByteBuffer(_), "length") => {
            let [] = exact_args(args, call_span)?;
            return builtin_byte_buffer_length(recv, call_span);
        }
        (Value::ByteBuffer(_), "get") => {
            let [index] = exact_args(args, call_span)?;
            return builtin_byte_buffer_get(recv, index, call_span);
        }
        (Value::ByteBuffer(_), "set") => {
            let [index, value] = exact_args(args, call_span)?;
            return builtin_byte_buffer_set(recv, index, value, call_span);
        }
        (Value::ByteBuffer(_), "fill") => {
            let [start, length, value] = exact_args(args, call_span)?;
            return builtin_byte_buffer_fill(recv, start, length, value, call_span);
        }
        (Value::ByteBuffer(_), "copy") => {
            let [source, source_start, target_start, length] = exact_args(args, call_span)?;
            return builtin_byte_buffer_copy(
                recv,
                source,
                source_start,
                target_start,
                length,
                call_span,
            );
        }
        (Value::ByteBuffer(_), "toBytes") => {
            let [] = exact_args(args, call_span)?;
            return builtin_byte_buffer_to_bytes(recv, call_span);
        }
        // §10 (v5.4) `Path` INSTANCE methods — all route through shared leaves.
        (Value::Path(_), "join") => {
            let [child] = exact_args(args, call_span)?;
            return builtin_path_join(recv, child, call_span);
        }
        (Value::Path(_), "parent") => {
            let [] = exact_args(args, call_span)?;
            return builtin_path_parent(recv, call_span);
        }
        (Value::Path(_), "fileName") => {
            let [] = exact_args(args, call_span)?;
            return builtin_path_file_name(recv, call_span);
        }
        (Value::Path(_), "extension") => {
            let [] = exact_args(args, call_span)?;
            return builtin_path_extension(recv, call_span);
        }
        (Value::Path(_), "withExtension") => {
            let [ext] = exact_args(args, call_span)?;
            return builtin_path_with_extension(recv, ext, call_span);
        }
        (Value::Path(_), "normalize") => {
            let [] = exact_args(args, call_span)?;
            return builtin_path_normalize(recv, call_span);
        }
        (Value::Path(_), "toString") => {
            let [] = exact_args(args, call_span)?;
            return builtin_path_to_string(recv, call_span);
        }
        // §11 (v5.4) Regex instance methods — shared leaves keep interp/emit aligned.
        (Value::Regex(_), "isMatch") => {
            let [text] = exact_args(args, call_span)?;
            return builtin_regex_is_match(recv, text, call_span);
        }
        (Value::Regex(_), "find") => {
            let [text] = exact_args(args, call_span)?;
            return builtin_regex_find(recv, text, call_span);
        }
        (Value::Regex(_), "findAll") => {
            let [text] = exact_args(args, call_span)?;
            return builtin_regex_find_all(recv, text, call_span);
        }
        (Value::Regex(_), "split") => {
            let [text] = exact_args(args, call_span)?;
            return builtin_regex_split(recv, text, call_span);
        }
        (Value::Regex(_), "replaceAll") => {
            let [text, replacement] = exact_args(args, call_span)?;
            return builtin_regex_replace_all(recv, text, replacement, call_span);
        }
        // §16 (v5.4) URL value accessors — no networking, just parsed components.
        (Value::Url(_), "scheme") => {
            let [] = exact_args(args, call_span)?;
            return builtin_url_scheme(recv, call_span);
        }
        (Value::Url(_), "host") => {
            let [] = exact_args(args, call_span)?;
            return builtin_url_host(recv, call_span);
        }
        (Value::Url(_), "path") => {
            let [] = exact_args(args, call_span)?;
            return builtin_url_path(recv, call_span);
        }
        (Value::Url(_), "query") => {
            let [] = exact_args(args, call_span)?;
            return builtin_url_query(recv, call_span);
        }
        (Value::Url(_), "fragment") => {
            let [] = exact_args(args, call_span)?;
            return builtin_url_fragment(recv, call_span);
        }
        (Value::Url(_), "toString") => {
            let [] = exact_args(args, call_span)?;
            return builtin_url_to_string(recv, call_span);
        }
        // §13 (v5.4) Date value accessors — deterministic Gregorian math only.
        (Value::Date(_), "toIso") => {
            let [] = exact_args(args, call_span)?;
            return builtin_date_to_iso(recv, call_span);
        }
        (Value::Date(_), "addDays") => {
            let [days] = exact_args(args, call_span)?;
            return builtin_date_add_days(recv, days, call_span);
        }
        (Value::Date(_), "year") => {
            let [] = exact_args(args, call_span)?;
            return builtin_date_year(recv, call_span);
        }
        (Value::Date(_), "month") => {
            let [] = exact_args(args, call_span)?;
            return builtin_date_month(recv, call_span);
        }
        (Value::Date(_), "day") => {
            let [] = exact_args(args, call_span)?;
            return builtin_date_day(recv, call_span);
        }
        (Value::BigInt(_), "toString") => {
            let [radix] = exact_args(args, call_span)?;
            return builtin_bigint_to_string(recv, radix, call_span);
        }
        (Value::BigInt(_), "toInt") => {
            let [] = exact_args(args, call_span)?;
            return builtin_bigint_to_int(recv, call_span);
        }
        (Value::BigInt(_), "div") => {
            let [other] = exact_args(args, call_span)?;
            return builtin_bigint_div(recv, other, call_span);
        }
        (Value::BigInt(_), "mod") => {
            let [other] = exact_args(args, call_span)?;
            return builtin_bigint_mod(recv, other, call_span);
        }
        (Value::Decimal(_), "toString") => {
            let [] = exact_args(args, call_span)?;
            return builtin_decimal_to_string(recv, call_span);
        }
        (Value::Decimal(_), "scale") => {
            let [] = exact_args(args, call_span)?;
            return builtin_decimal_scale(recv, call_span);
        }
        (Value::Decimal(_), "toInt") => {
            let [] = exact_args(args, call_span)?;
            return builtin_decimal_to_int(recv, call_span);
        }
        (Value::Decimal(_), "round") => {
            let (scale, mode) = match args.len() {
                1 => {
                    let [scale] = exact_args(args, call_span)?;
                    (scale, rounding_mode_value(RoundingMode::HalfEven))
                }
                2 => {
                    let [scale, mode] = exact_args(args, call_span)?;
                    (scale, mode)
                }
                found => return Err(arity_fault("1..2", found, call_span)),
            };
            return builtin_decimal_round(recv, scale, mode, call_span);
        }
        (Value::Decimal(_), "div") => {
            let (other, scale, mode) = match args.len() {
                2 => {
                    let [other, scale] = exact_args(args, call_span)?;
                    (other, scale, rounding_mode_value(RoundingMode::HalfEven))
                }
                3 => {
                    let [other, scale, mode] = exact_args(args, call_span)?;
                    (other, scale, mode)
                }
                found => return Err(arity_fault("2..3", found, call_span)),
            };
            return builtin_decimal_div(recv, other, scale, mode, call_span);
        }
        // §1 `arr.get(i)` — `Some(elem)` in bounds, else `None`; a non-`int`
        // index faults GUARD_TYPE. (The non-mutating array method.)
        (Value::Array(items), "get") => {
            let [index] = exact_args(args, call_span)?;
            match index {
                Value::Int(i) => {
                    let items = items.borrow();
                    if i >= 0 && (i as usize) < items.len() {
                        Value::Some(Rc::new(items[i as usize].clone()))
                    } else {
                        Value::None
                    }
                }
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`arr.get` takes an `int`, found `{}`", other.kind()),
                        call_span,
                    ));
                }
            }
        }
        // §6 `map.get(k)` — `Some(v)` if present else `None`; an unhashable
        // key faults through the shared comparator.
        (Value::Map(map), "get") => {
            let [key] = exact_args(args, call_span)?;
            match map
                .borrow()
                .get_value(&key)
                .map_err(|e| cmp_guard(e, call_span))?
            {
                Some(v) => Value::Some(Rc::new(v)),
                None => Value::None,
            }
        }
        // §22 `m.getOr(k, default)` — the value at `k`, or `default` if absent. The
        // ergonomic `m.get(k) ?? default` (including code where
        // `m.get(k) ?? 0` recurred). Pure leaf both engines share.
        (Value::Map(map), "getOr") => {
            let [key, default] = exact_args(args, call_span)?;
            match map
                .borrow()
                .get_value(&key)
                .map_err(|e| cmp_guard(e, call_span))?
            {
                Some(v) => v,
                None => default,
            }
        }
        // §22 `m.containsKey(k)` — whether the map has an entry at `k` (the
        // membership query without unwrapping an Option). Pure leaf both engines share.
        (Value::Map(map), "containsKey") => {
            let [key] = exact_args(args, call_span)?;
            Value::Bool(
                map.borrow()
                    .get_value(&key)
                    .map_err(|e| cmp_guard(e, call_span))?
                    .is_some(),
            )
        }
        // §1 `s.scalars()` — the string's Unicode scalar values as an array of
        // single-scalar strings.
        (Value::Str(s), "scalars") => {
            let [] = exact_args(args, call_span)?;
            Value::array(s.chars().map(|c| Value::str(c.to_string())).collect())
        }
        // §22 string stdlib (C3a) — read-only, operating on the Unicode SCALAR view
        // (consistent with `.scalars()`). `indexOf` returns a SCALAR index, never a byte
        // offset. ASCII whitespace = space/tab/LF/CR.
        (Value::Str(s), "startsWith") => {
            let [prefix] = exact_args(args, call_span)?;
            let prefix = match prefix {
                Value::Str(p) => p,
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!(
                            "`str.startsWith` takes a `string`, found `{}`",
                            other.kind()
                        ),
                        call_span,
                    ));
                }
            };
            Value::Bool(s.starts_with(&*prefix))
        }
        (Value::Str(s), "endsWith") => {
            let [suffix] = exact_args(args, call_span)?;
            let suffix = match suffix {
                Value::Str(p) => p,
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`str.endsWith` takes a `string`, found `{}`", other.kind()),
                        call_span,
                    ));
                }
            };
            Value::Bool(s.ends_with(&*suffix))
        }
        (Value::Str(s), "contains") => {
            let [sub] = exact_args(args, call_span)?;
            let sub = match sub {
                Value::Str(p) => p,
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`str.contains` takes a `string`, found `{}`", other.kind()),
                        call_span,
                    ));
                }
            };
            Value::Bool(s.contains(&*sub))
        }
        (Value::Str(s), "indexOf") => {
            let [sub] = exact_args(args, call_span)?;
            let sub = match sub {
                Value::Str(p) => p,
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`str.indexOf` takes a `string`, found `{}`", other.kind()),
                        call_span,
                    ));
                }
            };
            match s.find(&*sub) {
                // byte offset → scalar index (count scalars before the match)
                Some(byte) => Value::Some(Rc::new(Value::Int(s[..byte].chars().count() as i64))),
                None => Value::None,
            }
        }
        (Value::Str(s), "lastIndexOf") => {
            let [sub] = exact_args(args, call_span)?;
            let sub = match sub {
                Value::Str(p) => p,
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!(
                            "`str.lastIndexOf` takes a `string`, found `{}`",
                            other.kind()
                        ),
                        call_span,
                    ));
                }
            };
            match s.rfind(&*sub) {
                // byte offset -> scalar index (count scalars before the match)
                Some(byte) => Value::Some(Rc::new(Value::Int(s[..byte].chars().count() as i64))),
                None => Value::None,
            }
        }
        // §22 `str.codePointAt(i)` — the Unicode scalar VALUE at SCALAR index `i` (not byte),
        // `None` for i<0 or i>=scalar length. The char→codepoint primitive pure Topaz lacked
        // (enables Hangul 초성 = (cp-0xAC00)/588). Routed here so interp ≡ emit.
        (Value::Str(s), "codePointAt") => {
            let [index] = exact_args(args, call_span)?;
            let i = match index {
                Value::Int(n) => n,
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`str.codePointAt` takes an `int`, found `{}`", other.kind()),
                        call_span,
                    ));
                }
            };
            if i < 0 {
                Value::None
            } else {
                // `usize::try_from` (not `i as usize`) — a huge positive int must yield None,
                // not a truncated index, on a 32-bit usize (wasm32).
                match usize::try_from(i).ok().and_then(|idx| s.chars().nth(idx)) {
                    Some(c) => Value::Some(Rc::new(Value::Int((c as u32) as i64))),
                    None => Value::None,
                }
            }
        }
        (Value::Str(s), "trim") => {
            let [] = exact_args(args, call_span)?;
            Value::str(s.trim_matches([' ', '\t', '\n', '\r']))
        }
        (Value::Str(s), "trimStart") => {
            let [] = exact_args(args, call_span)?;
            Value::str(s.trim_start_matches([' ', '\t', '\n', '\r']))
        }
        (Value::Str(s), "trimEnd") => {
            let [] = exact_args(args, call_span)?;
            Value::str(s.trim_end_matches([' ', '\t', '\n', '\r']))
        }
        // §22 string stdlib: the UTF-8 BYTE length (Rust `str::len`), distinct from
        // `.scalars().length` (the scalar count). Useful for byte-limited forms (Hangul
        // is 3 bytes/scalar in UTF-8). Pure/deterministic; no normalization.
        (Value::Str(s), "byteLength") => {
            let [] = exact_args(args, call_span)?;
            Value::Int(s.len() as i64)
        }
        (Value::Str(s), "split") => {
            let [separator] = exact_args(args, call_span)?;
            let sep = match separator {
                Value::Str(p) => p,
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`str.split` takes a `string`, found `{}`", other.kind()),
                        call_span,
                    ));
                }
            };
            if sep.is_empty() {
                return Err(fault(
                    codes::GUARD_TYPE,
                    "`str.split` needs a non-empty separator; use `.scalars()` for a scalar split"
                        .to_string(),
                    call_span,
                ));
            }
            Value::array(s.split(&*sep).map(Value::str).collect())
        }
        // §22 `str.slice(start, end)` — the half-open `[start, end)` substring by
        // SCALAR index (like `.scalars()`/`.codePointAt`, NOT byte offset), CLAMPED
        // to the scalar bounds (`start` to `[0, len]`, `end` to `[start, len]`), so an
        // out-of-range or inverted range yields a shorter/empty string, never a fault.
        // A non-`int` bound faults `GUARD_TYPE`. Mirrors `arr.slice`.
        (Value::Str(s), "slice") => {
            let [start, end] = exact_args(args, call_span)?;
            let bound = |v: Value| match v {
                Value::Int(n) => Ok(n),
                other => Err(fault(
                    codes::GUARD_TYPE,
                    format!("`str.slice` takes `int` bounds, found `{}`", other.kind()),
                    call_span,
                )),
            };
            let start = bound(start)?;
            let end = bound(end)?;
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            let st = start.clamp(0, len);
            let en = end.clamp(st, len);
            Value::str(chars[st as usize..en as usize].iter().collect::<String>())
        }
        (Value::Str(s), "replace") => {
            let [needle, replacement] = exact_args(args, call_span)?;
            let replacement = match replacement {
                Value::Str(p) => p,
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!(
                            "`str.replace` takes a `string` replacement, found `{}`",
                            other.kind()
                        ),
                        call_span,
                    ));
                }
            };
            let needle = match needle {
                Value::Str(p) => p,
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!(
                            "`str.replace` takes a `string` needle, found `{}`",
                            other.kind()
                        ),
                        call_span,
                    ));
                }
            };
            Value::str(s.replace(&*needle, &replacement))
        }
        // §22 `n.atLeast(m)` = max(n,m) (floor) / `n.atMost(m)` = min(n,m) (ceiling) — clamp
        // building blocks (including `if v>0 {v-1} else {0}`). Pure
        // leaves both engines share. A non-int arg faults GUARD_TYPE.
        (Value::Int(n), "atLeast") => {
            let [other] = exact_args(args, call_span)?;
            let other = match other {
                Value::Int(m) => m,
                o => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`int.atLeast` takes an `int`, found `{}`", o.kind()),
                        call_span,
                    ));
                }
            };
            Value::Int((*n).max(other))
        }
        (Value::Int(n), "atMost") => {
            let [other] = exact_args(args, call_span)?;
            let other = match other {
                Value::Int(m) => m,
                o => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`int.atMost` takes an `int`, found `{}`", o.kind()),
                        call_span,
                    ));
                }
            };
            Value::Int((*n).min(other))
        }
        // §22.2 `opt.okOr(error)` — the EAGER Option→Result bridge. The `error`
        // argument is already evaluated (a value), so this is a pure leaf both
        // engines share: `Some(v)->Ok(v)` (the argument is unused), `None->Err(error)`.
        // (`okOrElse` is NOT here — it must call a closure, which needs the engine's
        // eval loop.)
        (Value::Some(v), "okOr") => {
            let [_error] = exact_args(args, call_span)?;
            Value::Ok(v.clone())
        }
        (Value::None, "okOr") => {
            let [error] = exact_args(args, call_span)?;
            Value::Err(Rc::new(error))
        }
        // §9/§22.2 the in-place MUTATORS — the SHARED leaf both engines call
        // (the interpreter's `call_builtin` delegates here, exactly like
        // `get`/`scalars`, so a bound-method call cannot drift). `member_access`
        // (interpreter) / the `let mut` receiver gate (emitter) already proved the
        // `mut`-root, so these are reached only on the right collection; the
        // mutation and the unhashable-key faults fall at the CALL span. `remove`
        // is the one mutator both `Map` and `Set` own — the receiver type, not the
        // method name, disambiguates here.
        (Value::Array(items), "push") => {
            let [value] = exact_args(args, call_span)?;
            items.borrow_mut().push(value);
            Value::Unit
        }
        // §22 `arr.slice(start, end)` — the half-open `[start, end)` sub-array,
        // CLAMPED to the array bounds (`start` to `[0, len]`, `end` to `[start, len]`),
        // so an out-of-range or inverted range yields a shorter/empty array, never a
        // fault. A non-`int` bound faults `GUARD_TYPE`.
        (Value::Array(items), "slice") => {
            let [start, end] = exact_args(args, call_span)?;
            let bound = |v: Value| match v {
                Value::Int(n) => Ok(n),
                other => Err(fault(
                    codes::GUARD_TYPE,
                    format!("`arr.slice` takes `int` bounds, found `{}`", other.kind()),
                    call_span,
                )),
            };
            let start = bound(start)?;
            let end = bound(end)?;
            let items = items.borrow();
            let len = items.len() as i64;
            let s = start.clamp(0, len);
            let e = end.clamp(s, len);
            Value::array(items[s as usize..e as usize].to_vec())
        }
        // §22 `arr.join(sep)` — each element RENDERED (the shared `render`, as in
        // string interpolation) and joined by `sep`. A non-`string` `sep` faults.
        (Value::Array(items), "join") => {
            let [separator] = exact_args(args, call_span)?;
            let sep = match separator {
                Value::Str(s) => s,
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`arr.join` takes a `string`, found `{}`", other.kind()),
                        call_span,
                    ));
                }
            };
            let items = items.borrow();
            let parts: Vec<String> = items.iter().map(render).collect();
            Value::str(parts.join(&sep))
        }
        // §22 `arr.indexOf(x)` — `Some(i)` for the FIRST element equal to `x` (the
        // shared `values_equal`; an uncomparable element faults via the comparator),
        // else `None`.
        (Value::Array(items), "indexOf") => {
            let [needle] = exact_args(args, call_span)?;
            let items = items.borrow();
            let mut found = Value::None;
            for (i, it) in items.iter().enumerate() {
                if values_equal(it, &needle).map_err(|e| cmp_guard(e, call_span))? {
                    found = Value::Some(Rc::new(Value::Int(i as i64)));
                    break;
                }
            }
            found
        }
        // §22 `arr.sorted()` — a NEW array sorted ascending in NATURAL order, NON-mutating
        // (the receiver is unchanged; a fresh array always) and STABLE (equal elements keep
        // their input order). Elements order through the SHARED `values_compare` leaf — so
        // `int`/`string`/`float` AND order-comparable NOMINALs (record decl-order, enum
        // variant-then-payload, newtype base) all sort, identically interp≡emit. The CHECKER
        // now rejects a non-order-comparable element type statically (no more check-pass-
        // then-fault), so a fault here is reachable only `--unchecked`: a non-orderable
        // element (or a mix of distinct kinds) faults GUARD_COMPARE (TPZ5007), the SAME
        // `cmp_guard` mapping `<` uses, at the first comparison that cannot decide.
        (Value::Array(items), "sorted") => {
            let [] = exact_args(args, call_span)?;
            let items = items.borrow();
            let mut out: Vec<Value> = items.to_vec();
            sort_values_stable(&mut out, call_span, values_compare)?;
            Value::array(out)
        }
        // §6 (v5.4) array mutation API — IN-PLACE through the shared `Rc<RefCell<Vec>>`
        // cell (the `mut`-root gate is the caller's static check + the interpreter's
        // `require_mut_root`/the emitter's mut-root gate). An aliased binding holding the
        // same `Rc` sees every change.
        //
        // `pop()` — remove + return the LAST element as `Option<T>` (`None` if empty).
        (Value::Array(items), "pop") => {
            let [] = exact_args(args, call_span)?;
            match items.borrow_mut().pop() {
                Some(v) => Value::Some(Rc::new(v)),
                None => Value::None,
            }
        }
        // `clear()` — empty in place; returns Unit.
        (Value::Array(items), "clear") => {
            let [] = exact_args(args, call_span)?;
            items.borrow_mut().clear();
            Value::Unit
        }
        // `reverse()` — reverse in place; returns Unit.
        (Value::Array(items), "reverse") => {
            let [] = exact_args(args, call_span)?;
            items.borrow_mut().reverse();
            Value::Unit
        }
        // `insert(index, value)` — insert `value` at `index`; an out-of-range index
        // (`index < 0 || index > length`, where `length` allows an append at the end)
        // FAULTS `FAULT_INDEX` (§6.5: invalid insert is a programmer bug). A non-`int`
        // index faults `GUARD_TYPE`. Returns Unit.
        (Value::Array(items), "insert") => {
            let [index, value] = exact_args(args, call_span)?;
            let index = match index {
                Value::Int(n) => n,
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!(
                            "`arr.insert` takes an `int` index, found `{}`",
                            other.kind()
                        ),
                        call_span,
                    ));
                }
            };
            let mut cell = items.borrow_mut();
            let len = cell.len() as i64;
            if index < 0 || index > len {
                return Err(fault(
                    codes::FAULT_INDEX,
                    format!("index {index} out of bounds for insert into length {len} (§6.5)"),
                    call_span,
                ));
            }
            cell.insert(index as usize, value);
            Value::Unit
        }
        // `removeAt(index)` — remove + return the element at `index` as `Option<T>`;
        // an out-of-range index yields `None` (§6.5, NOT a fault — distinct from
        // `insert`). A non-`int` index faults `GUARD_TYPE`.
        (Value::Array(items), "removeAt") => {
            let [index] = exact_args(args, call_span)?;
            let index = match index {
                Value::Int(n) => n,
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!(
                            "`arr.removeAt` takes an `int` index, found `{}`",
                            other.kind()
                        ),
                        call_span,
                    ));
                }
            };
            let mut cell = items.borrow_mut();
            let len = cell.len() as i64;
            if index < 0 || index >= len {
                Value::None
            } else {
                Value::Some(Rc::new(cell.remove(index as usize)))
            }
        }
        // `sort()` — sort IN PLACE ascending, the SAME `values_compare`/`sort_values_stable`
        // leaf `sorted` uses (so the order is byte-identical to `sorted`), written back into
        // the cell. A non-order-comparable element faults GUARD_COMPARE (reachable only
        // `--unchecked`; the checker rejects a non-orderable element). Returns Unit.
        (Value::Array(items), "sort") => {
            let [] = exact_args(args, call_span)?;
            // Snapshot, sort the copy, then write back — so a fault leaves the array
            // UNCHANGED and a `RefCell` re-borrow inside the comparator can't conflict.
            let mut out: Vec<Value> = items.borrow().to_vec();
            sort_values_stable(&mut out, call_span, values_compare)?;
            *items.borrow_mut() = out;
            Value::Unit
        }
        (Value::Map(map), "insert") => {
            let [key, value] = exact_args(args, call_span)?;
            map.borrow_mut()
                .insert_value(&key, value)
                .map_err(|e| cmp_guard(e, call_span))?;
            Value::Unit
        }
        (Value::Map(map), "remove") => {
            let [key] = exact_args(args, call_span)?;
            match map
                .borrow_mut()
                .remove_value(&key)
                .map_err(|e| cmp_guard(e, call_span))?
            {
                Some(v) => Value::Some(Rc::new(v)),
                None => Value::None,
            }
        }
        (Value::Set(set), "add") => {
            let [item] = exact_args(args, call_span)?;
            set.borrow_mut()
                .add_value(&item)
                .map_err(|e| cmp_guard(e, call_span))?;
            Value::Unit
        }
        (Value::Set(set), "remove") => {
            let [item] = exact_args(args, call_span)?;
            let changed = set
                .borrow_mut()
                .remove_value(&item)
                .map_err(|e| cmp_guard(e, call_span))?;
            Value::Bool(changed)
        }
        // §22 `s.contains(x)` — set membership as a method (mirrors `x in s`).
        // Pure leaf both engines share.
        (Value::Set(set), "contains") => {
            let [item] = exact_args(args, call_span)?;
            Value::Bool(
                set.borrow()
                    .contains_value(&item)
                    .map_err(|e| cmp_guard(e, call_span))?,
            )
        }
        // §6 (v5.4) `m.isEmpty()` / `s.isEmpty()` — whether the collection holds no
        // entries/elements. Pure leaf both engines share.
        (Value::Map(map), "isEmpty") => {
            let [] = exact_args(args, call_span)?;
            Value::Bool(map.borrow().is_empty())
        }
        (Value::Set(set), "isEmpty") => {
            let [] = exact_args(args, call_span)?;
            Value::Bool(set.borrow().is_empty())
        }
        // §6 (v5.4) `s.toArray()` — the elements as an `Array<T>` in insertion order
        // (a fresh snapshot, parallel to `map.keys`/`map.values`).
        (Value::Set(set), "toArray") => {
            let [] = exact_args(args, call_span)?;
            Value::array(set.borrow().items())
        }
        // §6 (v5.4) set ALGEBRA — non-mutating; a NEW `Set` in deterministic
        // insertion order (the leaf defines the order). The argument must be a `Set`
        // (the checker proves it; under `--unchecked` a non-Set faults GUARD_TYPE).
        (Value::Set(set), "union" | "intersection" | "difference") => {
            let [other] = exact_args(args, call_span)?;
            let other = match other {
                Value::Set(o) => o,
                bad => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`set.{method}` takes a `Set`, found `{}`", bad.kind()),
                        call_span,
                    ));
                }
            };
            let this = set.borrow();
            let that = other.borrow();
            let result = match method {
                "union" => this.union(&that),
                "intersection" => this.intersection(&that),
                _ => this.difference(&that),
            };
            Value::Set(Rc::new(RefCell::new(result)))
        }
        // §6 (v5.4) `m.clear()` / `s.clear()` — empties the collection IN PLACE through
        // the shared `Rc<RefCell>` cell (the `mut`-root gate is the caller's static
        // check + the interpreter's `require_mut_root`). Returns Unit, like `insert`/`add`.
        (Value::Map(map), "clear") => {
            let [] = exact_args(args, call_span)?;
            map.borrow_mut().clear();
            Value::Unit
        }
        (Value::Set(set), "clear") => {
            let [] = exact_args(args, call_span)?;
            set.borrow_mut().clear();
            Value::Unit
        }
        // Any other receiver/method pair has no such method — the same fault
        // the interpreter's `member_access` raises (at the MEMBER span).
        _ => return Err(no_member_fault(&recv, method, member_span)),
    })
}

/// Bind a generated receiver-method call through the canonical receiver builtin
/// identity, then enter [`call_method`]. The emitter performs member preflight
/// before evaluating these argument vectors, preserving callee-before-arguments
/// fault order while accepting the same named parameter catalog as the
/// interpreter's receiver-bound [`Value::Builtin`].
pub fn call_method_named(
    recv: Value,
    method: &str,
    positional: Vec<Value>,
    named: Vec<(String, Value)>,
    member_span: Span,
    call_span: Span,
) -> Result<Value, RtError> {
    let Some(receiver) = receiver_builtin(&recv, method)
        .filter(|receiver| receiver.route == ReceiverBuiltinRoute::Method)
    else {
        return Err(no_member_fault(&recv, method, member_span));
    };
    let args = bind_builtin_named_args(receiver.kind, true, positional, named, call_span)?;
    call_method(recv, method, args, member_span, call_span)
}

/// §22.3 the bound RESOURCE methods `file.read()` / `file.write(s)` /
/// `file.close()` — the SHARED leaf both engines call through the `Host`
/// (the interpreter passes `self.host`, the emitter `&*cx.host()`), so the
/// effect boundary cannot drift. `member_access` (interpreter) / the
/// member-call dispatch (emitter) already proved a `Value::Resource`
/// receiver, so the no-method arm is the wrong-receiver fault at the MEMBER
/// span; the arity/type faults fall at the CALL span. `read`→`Ok(str)/Err`,
/// `write`→`Ok(Unit)/Err` (a non-string arg faults `GUARD_TYPE`),
/// `close`→`Unit`.
pub fn call_resource_method(
    host: &dyn Host,
    recv: Value,
    method: &str,
    args: Vec<Value>,
    member_span: Span,
    call_span: Span,
) -> Result<Value, RtError> {
    let Value::Resource(h) = &recv else {
        return Err(no_member_fault(&recv, method, member_span));
    };
    Ok(match method {
        "read" => {
            let [] = exact_args(args, call_span)?;
            match host.read(*h) {
                Ok(s) => Value::Ok(Rc::new(Value::str(s))),
                Err(e) => Value::Err(Rc::new(Value::str(e))),
            }
        }
        "write" => {
            let [text] = exact_args(args, call_span)?;
            match text {
                Value::Str(s) => match host.write(*h, &s) {
                    Ok(()) => Value::Ok(Rc::new(Value::Unit)),
                    Err(e) => Value::Err(Rc::new(Value::str(e))),
                },
                other => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!("`file.write` takes a `string`, found `{}`", other.kind()),
                        call_span,
                    ));
                }
            }
        }
        "close" => {
            let [] = exact_args(args, call_span)?;
            host.close(*h);
            Value::Unit
        }
        _ => return Err(no_member_fault(&recv, method, member_span)),
    })
}

/// Bind named arguments for a bound RESOURCE method through the same builtin
/// parameter catalog used by the interpreter, then enter the shared host leaf.
/// The generated backend calls this only after its member preflight, so a wrong
/// receiver still faults before any argument expression is evaluated.
pub fn call_resource_method_named(
    host: &dyn Host,
    recv: Value,
    method: &str,
    positional: Vec<Value>,
    named: Vec<(String, Value)>,
    member_span: Span,
    call_span: Span,
) -> Result<Value, RtError> {
    let Some(receiver) = receiver_builtin(&recv, method)
        .filter(|receiver| receiver.route == ReceiverBuiltinRoute::Resource)
    else {
        return Err(no_member_fault(&recv, method, member_span));
    };
    let args = bind_builtin_named_args(receiver.kind, true, positional, named, call_span)?;
    call_resource_method(host, recv, method, args, member_span, call_span)
}

/// §8/§22 the receiver PREFLIGHT for a bound-method call — does `recv` carry
/// the builtin method `method`? Mirrors the interpreter's `member_access`
/// bound-method arms (the `(type, method)` pairs after `member_value`), so the
/// emitter can fault a wrong-receiver `no_member` at the MEMBER span BEFORE it
/// evaluates the call's arguments — exactly the interpreter's order
/// (`schedule_call` resolves the callee, then the args). Without it the
/// generated `call_method`/`call_resource_method` only rejects the receiver
/// AFTER the arg vec is built, so a side-effecting/faulting argument on a
/// wrong-type receiver (`5.write(print("x"))`) would diverge. The `mut`-root
/// requirement on the mutators is the emitter's separate static gate, not here.
pub fn check_member_method(recv: &Value, method: &str, member_span: Span) -> Result<(), RtError> {
    let supported = receiver_builtin(recv, method).is_some_and(|receiver| {
        matches!(
            receiver.route,
            ReceiverBuiltinRoute::Method | ReceiverBuiltinRoute::Resource
        ) || receiver.kind == Builtin::OkOrElse
    });
    if supported {
        Ok(())
    } else {
        Err(no_member_fault(recv, method, member_span))
    }
}

/// A §16 tagged-template value: tag + decoded literal parts + evaluated
/// interpolations, plus the `p`-tag platform-normalized text. SHARED so both
/// engines build and render it identically (CDR-006). Inert in v0.3 (no
/// execution; only `p` normalizes, the other tags render the diagnostic form).
#[derive(Debug)]
pub struct TemplateData {
    pub tag: String,
    /// Decoded literal text runs (n+1 parts around n interpolation values).
    pub parts: Vec<String>,
    pub values: Vec<Value>,
    /// `p` templates: the platform-normalized assembled text (§16); empty for
    /// the other tags.
    pub normalized: String,
}

impl TpzTemplate for TemplateData {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn render_into(&self, out: &mut String) {
        // Stable diagnostic form (CDR-003 §8): `p` renders its normalized text;
        // the other tags never text-insert values.
        if self.tag == "p" {
            out.push_str(&self.normalized);
        } else {
            out.push_str(&format!(
                "<{} template, {} part(s), {} interpolation(s)>",
                self.tag,
                self.parts.len(),
                self.values.len()
            ));
        }
    }
}

/// §16 build a tagged-template value — the SHARED leaf both engines call (the
/// interpreter's `continue_template` and the emitter), so the assembled
/// `p`-normalized text and the diagnostic rendering cannot drift. `p` applies
/// the reference platform normalization (`\` → `/`) to the assembled text
/// (parts interleaved with the rendered values); the other tags are inert.
pub fn make_template(tag: String, parts: Vec<String>, values: Vec<Value>) -> Value {
    let normalized = if tag == "p" {
        let mut text = String::new();
        for (k, part) in parts.iter().enumerate() {
            text.push_str(part);
            if let Some(v) = values.get(k) {
                text.push_str(&render(v));
            }
        }
        text.replace('\\', "/")
    } else {
        String::new()
    };
    Value::Template(Rc::new(TemplateData {
        tag,
        parts,
        values,
        normalized,
    }))
}
