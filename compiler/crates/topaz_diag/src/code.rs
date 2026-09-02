//! TPZ diagnostic-code registry.
//!
//! Reserved ranges (CDR-001 §5):
//!
//! | Range     | Producer                                    |
//! | --------- | ------------------------------------------- |
//! | `TPZ0xxx` | raw lexer / template lexer                  |
//! | `TPZ1xxx` | layout normalizer                           |
//! | `TPZ2xxx` | parser                                      |
//! | `TPZ3xxx` | resolver (`topaz_resolve`)                  |
//! | `TPZ4xxx` | runtime faults (`topaz_value` / interp)     |
//! | `TPZ5xxx` | static / dynamic semantic guards            |
//! | `TPZ6xxx` | native emitter / `topaz build`              |
//!
//! Per CDR-001 §5, **stable codes are assigned only to diagnostics
//! covered by `corpus/v5.1/invalid/` fixtures** — the ranges are
//! reserved here, but no code constant exists until its fixture
//! lands. Code constants live next to their producers (in
//! `topaz_lexer` / `topaz_parser`) and are constructed through
//! [`Code::new`].
//!
//! Two producer classes cannot use an ordinary invalid-language corpus row:
//! `TPZ58xx` reports a selected usage profile narrowing a canonically valid
//! program, and `TPZ6xxx` reports a native-emitter capability limit on a
//! WELL-TYPED program. Profile codes are pinned by profile CLI/collector tests;
//! emitter codes are pinned by the `emit`/`build` CLI tests and the
//! interpreter/emitter differential harness (CDR-006 §7). Their constants live
//! beside their producers.

/// A stable diagnostic code such as `TPZ2001`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Code(&'static str);

impl Code {
    /// Creates a code after enforcing the public `TPZ` + four-digit shape.
    pub const fn new(code: &'static str) -> Self {
        assert!(
            Self::has_registry_shape(code),
            "diagnostic code must match TPZ####"
        );
        Self(code)
    }

    pub fn as_str(&self) -> &'static str {
        self.0
    }

    /// Whether text has the public diagnostic-code shape.
    pub const fn has_registry_shape(code: &str) -> bool {
        let bytes = code.as_bytes();
        if bytes.len() != 7 || bytes[0] != b'T' || bytes[1] != b'P' || bytes[2] != b'Z' {
            return false;
        }
        let mut index = 3;
        while index < bytes.len() {
            if bytes[index] < b'0' || bytes[index] > b'9' {
                return false;
            }
            index += 1;
        }
        true
    }
}

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_shape_is_exact() {
        assert!(Code::has_registry_shape("TPZ0001"));
        assert!(Code::has_registry_shape("TPZ1042"));
        assert!(Code::has_registry_shape("TPZ2999"));
        assert!(!Code::has_registry_shape("TPZ001"));
        assert!(!Code::has_registry_shape("tpz0001"));
        assert!(!Code::has_registry_shape("TPZ00A1"));
        assert!(!Code::has_registry_shape("TPZ00001"));
    }

    #[test]
    #[should_panic(expected = "diagnostic code must match TPZ####")]
    fn constructor_rejects_malformed_codes() {
        let _ = Code::new("TPZ00A1");
    }
}

/// The static-semantics guard codes (TPZ5001-5008). One table feeds
/// both producers: the interpreter raises them as dynamic guards and
/// the checker graduates the same identities to compile time
/// (CDR-004 §6), so the two can never drift apart.
pub mod guard_codes {
    pub const TYPE: &str = "TPZ5001";
    pub const UNBOUND: &str = "TPZ5002";
    pub const IMMUTABLE: &str = "TPZ5003";
    pub const ARITY: &str = "TPZ5004";
    pub const NOT_CALLABLE: &str = "TPZ5005";
    pub const NO_FIELD: &str = "TPZ5006";
    pub const COMPARE: &str = "TPZ5007";
    pub const REDECLARE: &str = "TPZ5008";
    /// §4 the call-depth (recursion) limit guard — both engines fault here when
    /// nested Topaz calls exceed `CALL_DEPTH_LIMIT`, so the interpreter's heap frame
    /// stack and the emitted native stack agree instead of diverging (run silently
    /// succeeds vs build overflows the native stack).
    pub const RECURSION: &str = "TPZ5009";
}

/// Reserved v5.4 extern-FFI semantic diagnostics.
///
/// These are intentionally not emitted by `topaz_package`: manifest parsing
/// currently reports plain package errors and does not depend on `topaz_diag`.
/// The resolver/CLI extern seam maps the same manifest validation conditions to
/// these codes when they enter the normal diagnostic surface.
pub mod extern_codes {
    pub const DECL: &str = "TPZ5030";
    pub const ABI_TYPE: &str = "TPZ5031";
    pub const REPLAY: &str = "TPZ5032";
}
