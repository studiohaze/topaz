//! The native emitter's diagnostic-code registry (CDR-001 §5, `TPZ6xxx`).
//!
//! The `TPZ6xxx` range belongs to the NATIVE EMITTER / `topaz build` path. Unlike
//! the lexer/parser/resolver ranges — whose stable codes are pinned by
//! `corpus/v5.1/invalid/` fixtures (programs the language REJECTS) — a `TPZ6xxx`
//! code reports a capability limit of the emitter on a WELL-TYPED program the
//! language accepts. It is therefore pinned by the `emit`/`build` CLI tests and the
//! interpreter/emitter differential harness (CDR-006 §7), NOT by the invalid corpus.
//!
//! There is ONE umbrella code: the set of not-yet-lowerable constructs is
//! transitional and shrinks as coverage grows, so per-construct codes would churn.
//! The specific construct travels in the diagnostic MESSAGE, not the code.

use topaz_diag::Code;

/// `TPZ6001` — the native compiler cannot lower this construct yet. A coverage
/// refusal on a well-typed program, located at the offending node, with a "still
/// runs under `topaz run`" remedy. (Some refusals here MIRROR a semantic guard the
/// checker/interpreter owns long-term as `TPZ5xxx`; until the native path grows its
/// own static pass, the umbrella reports them as a coverage gap.)
pub const UNSUPPORTED_CONSTRUCT: Code = Code::new("TPZ6001");

/// `TPZ6002` — the v5.4 NATIVE (monomorphized) backend declined to lower this
/// program: a shape outside the bare-scalar island, or one it cannot prove
/// byte-identical to the interpreter (the `run≡build` invariant). UNLIKE
/// `TPZ6001`, a `TPZ6002` is NEVER user-facing: the CLI/harness ALWAYS falls
/// back to the boxed backend on it (which then either succeeds or raises its own
/// `TPZ6001`). It exists so a native refusal is a STRUCTURED outcome the parity
/// harness can pin separately — never a leaked `rustc` error from a divergent
/// emit. The declined construct travels in the message, not the code.
pub const NATIVE_DECLINED: Code = Code::new("TPZ6002");
