//! `topaz_interp` — the Topaz reference interpreter built on the CDR-003
//! baseline and the current language-mode contracts.
//!
//! It provides the `Host` effect boundary, the runtime value model, the
//! frame-stack evaluation machine, prelude dispatch, faults and guards,
//! deferred actions, cooperative concurrency, and module execution. The
//! native standard-library host remains in the `topaz_host_native` leaf.
//!
//! The core stays WASM-compatible: no direct filesystem, I/O, clock,
//! or thread use — everything observable crosses [`Host`].

mod host;
pub mod machine;
mod value;

pub use host::{Host, ResourceId, TestHost};
pub use machine::{Machine, RtError, RunResult};
/// The shared transcript comparator (CDR-006 §3): every harness —
/// the execution corpus, the CLI gate, and the future differential
/// harnesses — compares through this one implementation.
pub use topaz_value::transcript;
pub use value::{
    ClosureBody, ClosureData, ClosureParams, CmpError, Key, OrderedMap, OrderedSet, Value,
    canonical_key, key_to_value, render, values_equal,
};
