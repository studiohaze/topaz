//! Structural subtyping (CDR-004 §3).
//!
//! - `Literal(l) <: prim-of(l)`; `T <: Union[... T ...]`.
//! - Records are exact-shape: same field set, depth covariance.
//! - `Array`/`Map`/`Set` arguments are invariant; `Option`/`Result`
//!   are covariant.
//! - Function parameters are contravariant, returns covariant.

use crate::ty::Type;

pub fn is_subtype(sub: &Type, sup: &Type) -> bool {
    if sub == sup {
        return true;
    }
    match (sub, sup) {
        // Unknowns admit everything (staged checking, CDR-004 §2);
        // inference vars are handled by unification, never here.
        (Type::Unknown, _) | (_, Type::Unknown) => true,
        // A union is a subtype when every member is.
        (Type::Union(members), _) => members.iter().all(|m| is_subtype(m, sup)),
        // A non-union is a subtype of a union when some member
        // admits it.
        (_, Type::Union(members)) => members.iter().any(|m| is_subtype(sub, m)),
        (Type::Literal(lit), Type::Prim(p)) => lit.prim() == Some(*p),
        (Type::Record(a), Type::Record(b)) => {
            // Exact field set (width subtyping rejected: record
            // equality reasons over field sets, SPEC §2); fields are
            // sorted at formation, so pairwise walk suffices.
            a.len() == b.len()
                && a.iter()
                    .zip(b.iter())
                    .all(|((an, at), (bn, bt))| an == bn && is_subtype(at, bt))
        }
        (Type::Ctor(ca, aa), Type::Ctor(cb, ab)) => {
            ca == cb
                && aa.len() == ab.len()
                && if ca.covariant() {
                    aa.iter().zip(ab.iter()).all(|(x, y)| is_subtype(x, y))
                } else {
                    aa == ab
                }
        }
        (
            Type::Func {
                params: pa,
                variadic: va,
                ret: ra,
            },
            Type::Func {
                params: pb,
                variadic: vb,
                ret: rb,
            },
        ) => {
            pa.len() == pb.len()
                && pa
                    .iter()
                    .zip(pb.iter())
                    .all(|(x, y)| is_subtype(y, x)) // contravariant
                && match (va, vb) {
                    (None, None) => true,
                    (Some(x), Some(y)) => is_subtype(y, x),
                    _ => false,
                }
                && is_subtype(ra, rb) // covariant
        }
        _ => false,
    }
}
