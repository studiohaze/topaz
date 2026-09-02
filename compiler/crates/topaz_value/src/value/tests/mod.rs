//! Runtime-value coverage for numeric, JSON, ABI, and builtin behavior.
//! Shared spans and replay fixtures keep the leaf tests aligned across the two
//! execution engines.

use super::*;

// --- Scalar checked-arith leaf (Part A, v5.4 native-emit substrate) ---
const SP: Span = Span {
    file: topaz_diag::FileId(0),
    lo: 0,
    hi: 1,
};

const EXTERN_REPLAY_JSONL: &str = r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"42"}}"#;

mod abi;
mod builtins;
mod core;
mod json;
mod numeric;
