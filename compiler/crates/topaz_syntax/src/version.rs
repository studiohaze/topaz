macro_rules! language_version_registry {
    (
        $(
            $(#[$variant_meta:meta])*
            $variant:ident => $spelling:literal
        ),+ $(,)?
    ) => {
        /// Topaz language version selected for a parse session (CDR-002 §1).
        ///
        /// The version is a build/session input — never a per-file pragma and
        /// never auto-detected. Unmarked single-file CLI input is permanently pinned
        /// to the 5.16 profile; this library default stays `V5_1` for the
        /// `parse(file, src)` convenience and the version-pinned corpus harnesses.
        ///
        /// The variants are declared in release order, so derived ordering follows
        /// language inheritance. A feature of edition X is gated `>= X` so every
        /// later version inherits it: v5.2 syntax `>= V5_2`, user enums `>= V5_3`,
        /// multi-payload/recursive enums `>= V5_4`. Version parsing and stringification
        /// use exact-version equality.
        #[repr(u8)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
        pub enum LangVersion {
            $(
                $(#[$variant_meta])*
                $variant
            ),+
        }

        impl LangVersion {
            /// Canonical implemented-version and spelling table in release order.
            const ENTRIES: &'static [(Self, &'static str)] = &[
                $((Self::$variant, $spelling)),+
            ];

            /// Every implemented language line in release order.
            ///
            /// Ordering is language inheritance authority, so consumers that need the
            /// complete denominator use this iterator instead of repeating variants.
            pub fn all() -> impl ExactSizeIterator<Item = Self> {
                Self::ENTRIES.iter().map(|(version, _)| *version)
            }

            /// Exact manifest/CLI spelling without the `topaz-` prefix.
            pub const fn as_str(self) -> &'static str {
                Self::ENTRIES[self as usize].1
            }

            /// Parse one exact supported language-line spelling.
            pub fn parse_exact(raw: &str) -> Option<Self> {
                Self::ENTRIES
                    .iter()
                    .find_map(|(version, spelling)| (*spelling == raw).then_some(*version))
            }
        }
    };
}

language_version_registry! {
    /// The frozen v5.1 single-file language (`spec/v5.1/`).
    #[default]
    V5_1 => "5.1",
    /// The locked v5.2 language: modules plus the v5.2 base-syntax
    /// additions (`spec/v5.2/`). Strict superset of v5.1 for entry
    /// programs.
    V5_2 => "5.2",
    /// The v5.3 language: a strict superset of v5.2 (it inherits every
    /// v5.2 feature) plus user enums (`enum Name { … }`, §3) with
    /// payload-less and SINGLE-payload variants.
    V5_3 => "5.3",
    /// The v5.4 language: a strict superset of v5.3 plus MULTI-payload
    /// tuple variants and same-module RECURSIVE / mutually-recursive
    /// enums (`enum Expr { Num(int), Bin(Op, Expr, Expr) }`), nominal
    /// records (`record User { … }`), and newtypes (`newtype UserId = int`),
    /// available at v5.4 and later.
    V5_4 => "5.4",
    /// The v5.5 language/toolchain line: no new grammar gate over v5.4, but the
    /// public toolchain line where Python backend parity becomes a first-class
    /// release surface.
    V5_5 => "5.5",
    /// The v5.6 compatibility line, preserved with its complete public
    /// authority and implementation surface.
    V5_6 => "5.6",
    /// The v5.7 compatibility line. It inherits the complete v5.6 surface
    /// under the unified 5.7.0 product identity.
    V5_7 => "5.7",
    /// The v5.8 compatibility language/toolchain line. It inherits the complete v5.7
    /// surface and adds the lifecycle-v2 local data application contract.
    V5_8 => "5.8",
    /// The v5.9 compatibility language/toolchain identity. It inherits the complete
    /// v5.8 language surface and folds the bounded HTTP service host contract,
    /// and is available through current package and CLI selectors.
    V5_9 => "5.9",
    /// The v5.10 compatibility identity. It inherits the complete
    /// v5.9 language and product-authority surface and is available through
    /// current package and CLI selectors.
    V5_10 => "5.10",
    /// The v5.11 compatibility identity. It inherits the complete
    /// v5.10 language authority unchanged and exposes the Bootstrap Foundations
    /// product boundary through package and CLI selectors.
    V5_11 => "5.11",
    /// The v5.12 compatibility identity. It inherits the complete
    /// v5.11 language authority unchanged and exposes the explicit Self
    /// Front-end Preview while Rust Stage 0 remains the default and recovery
    /// engine.
    V5_12 => "5.12",
    /// The v5.13 compatibility identity. It inherits the complete
    /// v5.12 language authority unchanged and exposes the explicit Stage 1
    /// Compiler Preview while Rust Stage 0 remains the default and recovery
    /// engine.
    V5_13 => "5.13",
    /// The v5.14 compatibility identity. It inherits the complete
    /// v5.13 language authority unchanged and exposes the explicit Stage 2
    /// Fixed Point while Rust Stage 0 remains the default and recovery engine.
    V5_14 => "5.14",
    /// The v5.15 compatibility identity. It inherits the complete
    /// v5.14 language authority unchanged and exposes the Supported Dual
    /// Toolchain while Rust Stage 0 remains the default and recovery engine.
    V5_15 => "5.15",
    /// The v5.16 compatibility language/toolchain identity. It inherits the complete
    /// v5.15 language authority unchanged and makes the checked Stage 2
    /// compiler the default for supported current-mode routes. Rust Stage 0
    /// remains the explicit recovery and compatibility compiler.
    V5_16 => "5.16",
    /// The v5.17 compatibility language/toolchain identity. It inherits the complete
    /// v5.16 language authority unchanged and adds the installed bounded
    /// Lispex evaluator product without changing ordinary Topaz semantics.
    V5_17 => "5.17",
    /// The v5.18 compatibility language/toolchain identity. It inherits v5.17
    /// language semantics exactly and activates the separately gated
    /// first-class bounded Lispex application profile.
    V5_18 => "5.18",
    /// The v5.19 compatibility language/toolchain identity. It inherits v5.18
    /// language semantics exactly and activates the separately qualified
    /// complete-current-profile Lispex application product.
    V5_19 => "5.19",
    /// The current v5.20 language/toolchain identity. It introduces
    /// module-stable nominal declaration identity and imported typed-JSON
    /// schemas under ADR-131.
    V5_20 => "5.20",
}

impl LangVersion {
    /// Current public language line. Keep this separate from [`Default`] and
    /// [`Self::UNMARKED_SOURCE`]: library conveniences retain the frozen v5.1
    /// default, and unmarked single-file source remains pinned to 5.16 even
    /// after a future current profile advances.
    pub const CURRENT: Self = Self::V5_20;

    /// Permanent profile for unmarked single-file source.
    ///
    /// This is intentionally separate from [`Self::CURRENT`]. The current
    /// product line has advanced to 5.20 while unmarked source remains 5.16,
    /// so an existing file is never silently reinterpreted by a release.
    pub const UNMARKED_SOURCE: Self = Self::V5_16;

    /// Whether the installed self-hosted compiler is the product default for
    /// this language profile.
    ///
    /// Topaz 5.20 advances the complete 5.19 language authority.
    /// Keeping the admitted identities explicit preserves Self-hosted Default
    /// for existing unmarked files and 5.16-pinned packages without making any
    /// earlier language profile a self-hosted route. A future semantic profile
    /// must be admitted explicitly instead of widening this set by ordering.
    pub const fn uses_self_hosted_product_default(self) -> bool {
        matches!(
            self,
            Self::V5_16 | Self::V5_17 | Self::V5_18 | Self::V5_19 | Self::V5_20
        )
    }

    /// Whether this known language line is available through current public
    /// CLI and package selectors. Known future variants deliberately fail this
    /// gate until `CURRENT` advances.
    pub const fn is_selectable(self) -> bool {
        self as u8 <= Self::CURRENT as u8
    }

    /// Parse one exact language line only when the current product exposes it.
    pub fn parse_selectable(raw: &str) -> Option<Self> {
        Self::parse_exact(raw).filter(|version| version.is_selectable())
    }
}

#[cfg(test)]
mod tests {
    use super::LangVersion;

    #[test]
    fn exact_round_trip_and_current_are_explicit() {
        for (ordinal, version) in LangVersion::all().enumerate() {
            assert_eq!(version as usize, ordinal);
            assert_eq!(LangVersion::parse_exact(version.as_str()), Some(version));
        }
        assert_eq!(LangVersion::CURRENT, LangVersion::V5_20);
        assert_eq!(LangVersion::UNMARKED_SOURCE, LangVersion::V5_16);
        assert_ne!(LangVersion::UNMARKED_SOURCE, LangVersion::CURRENT);
        assert!(LangVersion::V5_16.uses_self_hosted_product_default());
        assert!(LangVersion::V5_17.uses_self_hosted_product_default());
        assert!(LangVersion::V5_18.uses_self_hosted_product_default());
        assert!(LangVersion::V5_19.uses_self_hosted_product_default());
        assert!(LangVersion::V5_20.uses_self_hosted_product_default());
        assert!(!LangVersion::V5_15.uses_self_hosted_product_default());
        assert_eq!(LangVersion::default(), LangVersion::V5_1);
        assert_eq!(LangVersion::parse_exact("topaz-5.6"), None);
        assert_eq!(LangVersion::parse_exact("5.20"), Some(LangVersion::V5_20));
        assert!(LangVersion::V5_20.is_selectable());
        assert_eq!(
            LangVersion::parse_selectable("5.20"),
            Some(LangVersion::CURRENT)
        );
    }
}
