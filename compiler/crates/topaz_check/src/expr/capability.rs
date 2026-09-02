use super::*;

pub(super) fn contains_byte_buffer_in(
    ty: &Type,
    enums: &HashMap<String, EnumInfo>,
    records: &HashMap<String, RecordInfo>,
    newtypes: &HashMap<String, NewtypeInfo>,
    seen: &mut Vec<String>,
) -> bool {
    match ty {
        Type::ByteBuffer => true,
        Type::Ctor(_, args) | Type::Foreign { args, .. } => args
            .iter()
            .any(|arg| contains_byte_buffer_in(arg, enums, records, newtypes, seen)),
        Type::Record(fields) => fields
            .iter()
            .any(|(_, field)| contains_byte_buffer_in(field, enums, records, newtypes, seen)),
        Type::Union(members) => members
            .iter()
            .any(|member| contains_byte_buffer_in(member, enums, records, newtypes, seen)),
        // A callable signature can mention ByteBuffer without the callable value
        // itself containing or rendering mutable byte storage.
        Type::Func { .. } => false,
        Type::Enum { base, args } => {
            if args
                .iter()
                .any(|arg| contains_byte_buffer_in(arg, enums, records, newtypes, seen))
            {
                return true;
            }
            let id = nominal_instance_id(base, args);
            if seen.iter().any(|item| item == &id) {
                return false;
            }
            let Some(info) = enums.get(&id) else {
                return false;
            };
            seen.push(id);
            let contains = info.variants.iter().any(|variant| {
                variant
                    .payloads
                    .iter()
                    .any(|payload| contains_byte_buffer_in(payload, enums, records, newtypes, seen))
            });
            seen.pop();
            contains
        }
        Type::NominalRecord { base, args } => {
            if args
                .iter()
                .any(|arg| contains_byte_buffer_in(arg, enums, records, newtypes, seen))
            {
                return true;
            }
            let id = nominal_instance_id(base, args);
            if seen.iter().any(|item| item == &id) {
                return false;
            }
            let Some(info) = records.get(&id) else {
                return false;
            };
            seen.push(id);
            let contains = info
                .fields
                .iter()
                .any(|field| contains_byte_buffer_in(&field.ty, enums, records, newtypes, seen));
            seen.pop();
            contains
        }
        Type::Newtype { base, args } => {
            if args
                .iter()
                .any(|arg| contains_byte_buffer_in(arg, enums, records, newtypes, seen))
            {
                return true;
            }
            let id = nominal_instance_id(base, args);
            if seen.iter().any(|item| item == &id) {
                return false;
            }
            let Some(info) = newtypes.get(&id) else {
                return false;
            };
            seen.push(id);
            let contains = contains_byte_buffer_in(&info.base, enums, records, newtypes, seen);
            seen.pop();
            contains
        }
        Type::Prim(_)
        | Type::Literal(_)
        | Type::Skolem { .. }
        | Type::Template
        | Type::File
        | Type::JsonValue
        | Type::Bytes
        | Type::Path
        | Type::Regex
        | Type::Match
        | Type::TomlValue
        | Type::Url
        | Type::Date
        | Type::BigInt
        | Type::Decimal
        | Type::RoundingMode
        | Type::Unknown
        | Type::Var(_) => false,
    }
}

/// SPEC §2 comparability: Map, Set, functions, files, and template
/// values are non-comparable; aggregates inherit non-comparability
/// from their members. Unknown components count as comparable so
/// staged checking stays silent.
/// SPEC §2 comparability, with the program's nominal-record table so a NOMINAL
/// record's comparability consults its DECLARED field types (runtime eq descends
/// the fields — value.rs — so a record with a `Map`/`Set`/function field is NOT
/// comparable). `seen` breaks a recursive-record cycle: a record referenced
/// while already being checked is treated as comparable (its other fields decide).
pub(crate) fn comparable_in(
    ty: &Type,
    enums: &HashMap<String, EnumInfo>,
    records: &HashMap<String, RecordInfo>,
    newtypes: &HashMap<String, NewtypeInfo>,
    seen: &mut Vec<String>,
) -> bool {
    match ty {
        Type::Template | Type::File | Type::JsonValue | Type::TomlValue => false,
        Type::Func { .. } => false,
        Type::Ctor(Ctor::Map | Ctor::Set, _) => false,
        Type::Ctor(_, args) => args
            .iter()
            .all(|a| comparable_in(a, enums, records, newtypes, seen)),
        Type::Record(fields) => fields
            .iter()
            .all(|(_, t)| comparable_in(t, enums, records, newtypes, seen)),
        Type::Union(ms) => ms
            .iter()
            .all(|m| comparable_in(m, enums, records, newtypes, seen)),
        // §3 an enum is comparable iff EVERY declared payload type is comparable:
        // runtime equality walks matching-variant payloads recursively, so a Map/Set/
        // function nested behind an enum tag would otherwise fault after CHECK.
        Type::Enum { base, args } => {
            let id = nominal_instance_id(base, args);
            if seen.iter().any(|s| s == &id) {
                return true; // recursive ref — its other payloads decide.
            }
            let Some(info) = enums.get(&id) else {
                return true; // unknown enum (e.g. imported) — stay permissive.
            };
            seen.push(id);
            let ok = info.variants.iter().all(|v| {
                v.payloads
                    .iter()
                    .all(|p| comparable_in(p, enums, records, newtypes, seen))
            });
            seen.pop();
            ok
        }
        Type::NominalRecord { base, args } => {
            let id = nominal_instance_id(base, args);
            if seen.iter().any(|s| s == &id) {
                return true; // recursive ref — its other fields decide.
            }
            let Some(info) = records.get(&id) else {
                return true; // unknown record (e.g. imported) — stay permissive.
            };
            seen.push(id);
            let ok = info
                .fields
                .iter()
                .all(|f| comparable_in(&f.ty, enums, records, newtypes, seen));
            seen.pop();
            ok
        }
        // §3 a newtype is comparable iff its BASE type is comparable (runtime `eq`
        // descends into the wrapped inner value — value.rs — so a newtype over a
        // `Map`/function base is NOT comparable). `seen` breaks a cycle for a
        // newtype whose base reaches itself (stay permissive).
        Type::Newtype { base, args } => {
            let id = nominal_instance_id(base, args);
            if seen.iter().any(|s| s == &id) {
                return true;
            }
            let Some(info) = newtypes.get(&id) else {
                return true; // unknown newtype (e.g. imported) — stay permissive.
            };
            seen.push(id);
            let ok = comparable_in(&info.base, enums, records, newtypes, seen);
            seen.pop();
            ok
        }
        // §8/§10/§11/§16 (v5.4) `Bytes`/`Path` are scalar-like; `URL` has
        // canonical-string Eq; `Match` has field-defined Eq. Opaque `Regex` is
        // intentionally not comparable.
        Type::Prim(_)
        | Type::Literal(_)
        | Type::Bytes
        | Type::Path
        | Type::Url
        | Type::Date
        | Type::BigInt
        | Type::Decimal
        | Type::RoundingMode
        | Type::Match
        | Type::Foreign { .. }
        | Type::Skolem { .. } => true,
        Type::ByteBuffer | Type::Regex => false,
        Type::Unknown | Type::Var(_) => true,
    }
}

/// SPEC §2 ORDER comparability (`<`/`<=`/`>`/`>=`, `.sorted()`, `.sortedBy` key):
/// STRUCTURAL and consistent with `==` — a nominal whose fields/payloads are ALL
/// order-comparable is itself order-comparable, with NO `derives(Order)` gate (derive
/// is for generic bounds, which are not supported here; `==` is already structural without
/// `derives(Eq)`, so ordering matches). The ORDER-comparable SCALARS are `int`,
/// `float`, and `string` ONLY — NOT `bool`/`unit`/`null` (which `==` admits but `<`
/// has never ordered), and NOT Option/Result/Array/Map/Set/JSONValue/function/File/
/// template (no specified total order). The NOMINAL kinds order by their components
/// (the runtime `values_compare` leaf walks them): a record by its fields in DECL
/// order, an enum by variant index then payloads L→R (so its payload types must ALL
/// be order-comparable — UNLIKE `==`, which orders enums by tag alone), a newtype by
/// its base. `seen` breaks a recursive nominal cycle (its other components decide).
/// The boolean view accepts only known-orderable types; concrete gates use the
/// tri-state helper below so unresolved inference vars can defer without admitting
/// opaque `Skolem`/`Foreign`/`Unknown` as orderable.
pub(crate) fn order_comparable_in(
    ty: &Type,
    enums: &HashMap<String, EnumInfo>,
    records: &HashMap<String, RecordInfo>,
    newtypes: &HashMap<String, NewtypeInfo>,
    seen: &mut Vec<String>,
) -> bool {
    order_comparable_gate(ty, enums, records, newtypes, seen) == GateCheck::Accept
}

pub(super) fn order_comparable_gate(
    ty: &Type,
    enums: &HashMap<String, EnumInfo>,
    records: &HashMap<String, RecordInfo>,
    newtypes: &HashMap<String, NewtypeInfo>,
    seen: &mut Vec<String>,
) -> GateCheck {
    match ty {
        // The order-comparable scalars (and their literal-type singletons). §8/§10
        // `Bytes`/`Path`/`URL` join them: each orders by its canonical
        // representation, so `<`/`sorted`/`sortedBy` accept it.
        Type::Prim(Prim::Int | Prim::Float | Prim::String)
        | Type::Bytes
        | Type::Path
        | Type::Url
        | Type::Date
        | Type::BigInt
        | Type::Decimal => GateCheck::Accept,
        Type::Literal(lit)
            if matches!(lit.prim(), Some(Prim::Int | Prim::Float | Prim::String)) =>
        {
            GateCheck::Accept
        }
        // NO specified total order for bool/unit, the containers, or the opaque types.
        Type::Literal(_) | Type::Prim(Prim::Bool | Prim::Unit) => GateCheck::Reject,
        Type::ByteBuffer
        | Type::Template
        | Type::File
        | Type::JsonValue
        | Type::TomlValue
        | Type::Func { .. } => GateCheck::Reject,
        Type::Regex | Type::Match | Type::RoundingMode => GateCheck::Reject,
        Type::Ctor(..) => GateCheck::Reject,
        Type::Record(_) | Type::Union(_) => GateCheck::Reject,
        // §3 a NOMINAL record is order-comparable iff EVERY declared field type is.
        Type::NominalRecord { base, args } => {
            let id = nominal_instance_id(base, args);
            if seen.iter().any(|s| s == &id) {
                return GateCheck::Accept; // recursive ref — its other fields decide.
            }
            let Some(info) = records.get(&id) else {
                return GateCheck::Accept; // unknown record (e.g. imported) — stay permissive.
            };
            seen.push(id);
            let ok = info.fields.iter().fold(GateCheck::Accept, |acc, f| {
                acc.and(order_comparable_gate(&f.ty, enums, records, newtypes, seen))
            });
            seen.pop();
            ok
        }
        // §3 an enum is order-comparable iff EVERY variant's EVERY payload type is
        // (variant index orders the tag; payloads order within a variant). A
        // payload-less enum (every variant `payloads` empty) is order-comparable by
        // tag alone. Consults the enum table for the payload types.
        Type::Enum { base, args } => {
            let id = nominal_instance_id(base, args);
            if seen.iter().any(|s| s == &id) {
                return GateCheck::Accept; // recursive ref — its other payloads decide.
            }
            let Some(info) = enums.get(&id) else {
                return GateCheck::Accept; // unknown enum (e.g. imported) — stay permissive.
            };
            seen.push(id);
            let ok = info.variants.iter().fold(GateCheck::Accept, |acc, v| {
                acc.and(v.payloads.iter().fold(GateCheck::Accept, |acc, p| {
                    acc.and(order_comparable_gate(p, enums, records, newtypes, seen))
                }))
            });
            seen.pop();
            ok
        }
        // §3 a newtype is order-comparable iff its BASE is.
        Type::Newtype { base, args } => {
            let id = nominal_instance_id(base, args);
            if seen.iter().any(|s| s == &id) {
                return GateCheck::Accept;
            }
            let Some(info) = newtypes.get(&id) else {
                return GateCheck::Accept; // unknown newtype (e.g. imported) — stay permissive.
            };
            seen.push(id);
            let ok = order_comparable_gate(&info.base, enums, records, newtypes, seen);
            seen.pop();
            ok
        }
        // A rigid/opaque generic parameter has no known order; an unresolved local
        // inference var can still be solved by surrounding inference, so defer it.
        Type::Foreign { .. } | Type::Skolem { .. } | Type::Unknown => GateCheck::Reject,
        Type::Var(_) => GateCheck::Defer,
    }
}

/// §22/§4 (v5.4) JSON encodability (`JSON.stringify`): STRUCTURAL and consistent
/// with the shared `encode_json` leaf (`topaz_value`) — a nominal whose
/// fields/payloads/base are ALL encodable is itself encodable. A `derives(JSON)`
/// conformance is additionally honored for rigid generic `T: JSON` arguments; direct
/// concrete calls remain structural for backward compatibility.
/// This is the CHECK-side mirror of `encode_json` so a non-encodable argument is
/// rejected at CHECK time (check==runtime), not silently passed to a runtime `Err`.
/// The ENCODABLE leaves match the leaf exactly: `int`/`bool`/`unit`/`null`/`string`,
/// a structural `Option` (encodable iff its element is — `Some(v)` is `v`, `None` is
/// `null`), an `Array` (element encodable), a structural `Record` (all fields), a
/// `Map<string, V>` (STRING keys only, `V` encodable), `JSONValue` (always). The
/// REJECTED types match the leaf's `Err` arms: `float` (canonicalization hazard),
/// `Result`/`Set`/`range`, a `Map` with a non-string key, `Func`/`File`/`Template`,
/// and a union (no single JSON shape). The NOMINAL kinds encode by their components:
/// a record by ALL its field types, an enum by ALL its variants' payload types, a
/// newtype by its base. `derives(JSON)` records conformance metadata but is not a
/// call-site gate for structural encoding. `seen` breaks a recursive-nominal cycle
/// (its other components decide). Opaque/generic shapes (`Foreign`/`Skolem`/`Unknown`) are NOT encodable;
/// a still-unresolved local inference `Var` defers after call-site substitution.
#[allow(dead_code)]
pub(crate) fn json_encodable_in(
    ty: &Type,
    enums: &HashMap<String, EnumInfo>,
    records: &HashMap<String, RecordInfo>,
    newtypes: &HashMap<String, NewtypeInfo>,
    seen: &mut Vec<String>,
) -> bool {
    json_encodable_status(ty, enums, records, newtypes, seen) == GateCheck::Accept
}

#[allow(dead_code)]
pub(crate) fn json_decodable_in(
    ty: &Type,
    enums: &HashMap<String, EnumInfo>,
    records: &HashMap<String, RecordInfo>,
    newtypes: &HashMap<String, NewtypeInfo>,
    seen: &mut Vec<String>,
) -> bool {
    json_decodable_status(ty, enums, records, newtypes, seen) == GateCheck::Accept
}

#[derive(Clone, Copy)]
enum JsonDirection {
    Encode,
    Decode,
}

pub(super) fn json_encodable_status(
    ty: &Type,
    enums: &HashMap<String, EnumInfo>,
    records: &HashMap<String, RecordInfo>,
    newtypes: &HashMap<String, NewtypeInfo>,
    seen: &mut Vec<String>,
) -> GateCheck {
    json_capability_status(ty, enums, records, newtypes, seen, JsonDirection::Encode)
}

pub(super) fn json_decodable_status(
    ty: &Type,
    enums: &HashMap<String, EnumInfo>,
    records: &HashMap<String, RecordInfo>,
    newtypes: &HashMap<String, NewtypeInfo>,
    seen: &mut Vec<String>,
) -> GateCheck {
    json_capability_status(ty, enums, records, newtypes, seen, JsonDirection::Decode)
}

/// Shared structural JSON capability traversal. Direction changes only the two
/// intentionally asymmetric shapes: `Match` is encode-only, and recursive nominal
/// cycles are encodable but cannot materialize a finite decoding schema.
fn json_capability_status(
    ty: &Type,
    enums: &HashMap<String, EnumInfo>,
    records: &HashMap<String, RecordInfo>,
    newtypes: &HashMap<String, NewtypeInfo>,
    seen: &mut Vec<String>,
    direction: JsonDirection,
) -> GateCheck {
    match ty {
        // Shared JSON scalars (and their literal-type singletons). `null` and `unit`
        // both use the leaf's `Unit | None | Null` representation.
        Type::Prim(Prim::Int | Prim::Bool | Prim::String | Prim::Unit) => GateCheck::Accept,
        Type::Literal(lit)
            if matches!(
                lit.prim(),
                Some(Prim::Int | Prim::Bool | Prim::String) | None // None = the `null` literal
            ) =>
        {
            GateCheck::Accept
        }
        // `float` is outside the supported canonical JSON shape.
        Type::Literal(_) | Type::Prim(Prim::Float) => GateCheck::Reject,
        Type::JsonValue => GateCheck::Accept,
        // Opaque types and no-single-shape forms are unsupported in both directions.
        // §8/§10 (v5.4) `Bytes` and `Path` require an explicit representation bridge.
        Type::Template
        | Type::File
        | Type::Bytes
        | Type::ByteBuffer
        | Type::Path
        | Type::Date
        | Type::BigInt
        | Type::Decimal
        | Type::RoundingMode
        | Type::Regex
        | Type::TomlValue
        | Type::Url
        | Type::Func { .. }
        | Type::Union(_) => GateCheck::Reject,
        Type::Match => match direction {
            JsonDirection::Encode => GateCheck::Accept,
            JsonDirection::Decode => GateCheck::Reject,
        },
        Type::Ctor(ctor, args) => match ctor {
            // Option and Array inherit the capability of their element shape.
            Ctor::Option | Ctor::Array => {
                json_capability_status(&args[0], enums, records, newtypes, seen, direction)
            }
            // A Map uses a JSON object shape: STRING keys only, with a capable value.
            Ctor::Map => {
                let key = match &args[0] {
                    Type::Prim(Prim::String) | Type::Literal(Lit::Str(_)) => GateCheck::Accept,
                    Type::Var(_) => GateCheck::Defer,
                    _ => GateCheck::Reject,
                };
                key.and(json_capability_status(
                    &args[1], enums, records, newtypes, seen, direction,
                ))
            }
            // Result, Set, and range have no JSON form in v1.
            Ctor::Result | Ctor::Set | Ctor::Range => GateCheck::Reject,
        },
        // A structural record is capable iff every field type is.
        Type::Record(fields) => fields.iter().fold(GateCheck::Accept, |acc, (_, t)| {
            acc.and(json_capability_status(
                t, enums, records, newtypes, seen, direction,
            ))
        }),
        // §3 a nominal record is capable iff every declared field type is.
        Type::NominalRecord { base, args } => {
            let id = nominal_instance_id(base, args);
            if seen.iter().any(|s| s == &id) {
                return match direction {
                    JsonDirection::Encode => GateCheck::Accept,
                    JsonDirection::Decode => GateCheck::Reject,
                };
            }
            let Some(info) = records.get(&id) else {
                return GateCheck::Accept; // unknown record (e.g. imported) — stay permissive.
            };
            seen.push(id);
            let ok = info.fields.iter().fold(GateCheck::Accept, |acc, f| {
                acc.and(json_capability_status(
                    &f.ty, enums, records, newtypes, seen, direction,
                ))
            });
            seen.pop();
            ok
        }
        // §3 an enum is capable iff every variant payload type is; a payload-less enum
        // uses its tag alone.
        Type::Enum { base, args } => {
            let id = nominal_instance_id(base, args);
            if seen.iter().any(|s| s == &id) {
                return match direction {
                    JsonDirection::Encode => GateCheck::Accept,
                    JsonDirection::Decode => GateCheck::Reject,
                };
            }
            let Some(info) = enums.get(&id) else {
                return GateCheck::Accept; // unknown enum (e.g. imported) — stay permissive.
            };
            seen.push(id);
            let ok = info.variants.iter().fold(GateCheck::Accept, |acc, v| {
                acc.and(v.payloads.iter().fold(GateCheck::Accept, |acc, p| {
                    acc.and(json_capability_status(
                        p, enums, records, newtypes, seen, direction,
                    ))
                }))
            });
            seen.pop();
            ok
        }
        // §3 a newtype follows its transparent base shape.
        Type::Newtype { base, args } => {
            let id = nominal_instance_id(base, args);
            if seen.iter().any(|s| s == &id) {
                return match direction {
                    JsonDirection::Encode => GateCheck::Accept,
                    JsonDirection::Decode => GateCheck::Reject,
                };
            }
            let Some(info) = newtypes.get(&id) else {
                return GateCheck::Accept; // unknown newtype (e.g. imported) — stay permissive.
            };
            seen.push(id);
            let ok = json_capability_status(&info.base, enums, records, newtypes, seen, direction);
            seen.pop();
            ok
        }
        // A rigid/opaque generic parameter has no known JSON shape. A local inference
        // var may still resolve through call substitution, so it defers here.
        Type::Foreign { .. } | Type::Skolem { .. } | Type::Unknown => GateCheck::Reject,
        Type::Var(_) => GateCheck::Defer,
    }
}

pub(super) fn type_has_var(ty: &Type) -> bool {
    ty.any_component(&mut |component| matches!(component, Type::Var(_)))
}

/// A typed JSON target must materialize a finite runtime descriptor from the
/// written type alone. Inference variables, rigid function parameters, and a
/// true unknown all make that descriptor open even though ordinary call
/// unification may otherwise tolerate them.
pub(super) fn type_has_schema_variable(ty: &Type) -> bool {
    ty.any_component(&mut |component| {
        matches!(
            component,
            Type::Var(_) | Type::Skolem { .. } | Type::Unknown
        )
    })
}
