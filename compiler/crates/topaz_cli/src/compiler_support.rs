use topaz_self_frontend::{InstalledStage2Identity, SELF_COMPILATION_PRODUCT_SCHEMA};

pub const SUPPORT_SCHEMA: &str = "topaz.compiler-support/v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Compiler implementation selectable at the CLI boundary.
pub enum CompilerSelection {
    Rust,
    SelfHosted,
}

impl CompilerSelection {
    /// Parses the stable `--compiler` selector spelling.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "rust" => Some(Self::Rust),
            "self" => Some(Self::SelfHosted),
            _ => None,
        }
    }

    pub const fn selector(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::SelfHosted => "self",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Omitted or explicit compiler choice before route policy is applied.
pub enum CompilerIntent {
    Omitted,
    Explicit(CompilerSelection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Policy source responsible for the resolved compiler choice.
pub enum SelectionOrigin {
    Explicit,
    CurrentDefault,
    Compatibility,
}

impl SelectionOrigin {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::CurrentDefault => "current-default",
            Self::Compatibility => "compatibility",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Selected compiler and the policy origin that chose it.
pub struct ResolvedCompilerSelection {
    pub selected_compiler: Option<CompilerSelection>,
    pub selection_origin: Option<SelectionOrigin>,
}

/// CLI facts needed to admit compiler selection before command execution.
pub struct PreflightRequest<'a> {
    pub intent: CompilerIntent,
    pub product_default: CompilerSelection,
    pub args: &'a [String],
    pub self_hosted_default_profile: bool,
    pub locked: bool,
    pub unchecked: bool,
    pub experimental: bool,
    pub profile: bool,
    pub exports_json: bool,
    pub backend_native: bool,
    pub native_report: bool,
    pub producer: bool,
    pub self_source: bool,
    pub target: Option<&'a str>,
}

fn is_compiler_bearing(args: &[String]) -> bool {
    matches!(
        args.first().map(String::as_str),
        Some(
            "parse"
                | "dump-ast"
                | "check"
                | "run"
                | "test"
                | "fmt"
                | "lsp"
                | "emit"
                | "build"
                | "dev"
                | "doc"
                | "refactor"
                | "migrate"
                | "bench"
                | "check-corpus"
        )
    ) || matches!(
        args,
        [compiler, observe, ..] if compiler == "compiler" && observe == "observe"
    )
}

fn self_decline(reason: &str) -> String {
    format!(
        "`--compiler self` {reason}; recovery: rerun the same command with `--compiler rust` (not executed)"
    )
}

fn declared_compatibility_route(request: &PreflightRequest<'_>) -> bool {
    !request.self_hosted_default_profile
        || request.unchecked
        || request.backend_native
        || request.native_report
        || matches!(
            request.args.first().map(String::as_str),
            Some("refactor" | "migrate" | "check-corpus")
        )
}

fn validate_self_selection(request: &PreflightRequest<'_>) -> Result<(), String> {
    if !request.self_hosted_default_profile {
        return Err(self_decline(
            "supports only an exact language profile admitted for the installed self-hosted product",
        ));
    }
    if request.unchecked {
        return Err(self_decline("does not support `--unchecked`"));
    }
    if request.backend_native || request.native_report {
        return Err(self_decline(
            "does not support native specialization or `--native-report-json`",
        ));
    }
    if request.experimental {
        return Err(self_decline("does not support legacy `--experimental`"));
    }
    match request.args {
        [command, _] if matches!(command.as_str(), "parse" | "dump-ast") => {
            if request.locked || request.profile || request.exports_json {
                return Err(self_decline(
                    "does not accept package or check modifiers on parse routes",
                ));
            }
            Ok(())
        }
        [command] | [command, _]
            if matches!(command.as_str(), "check" | "run" | "test" | "bench") =>
        {
            if request.profile && command != "check" {
                return Err(self_decline("accepts `--profile` only on `check`"));
            }
            if request.exports_json && command != "check" {
                return Err(self_decline("accepts `--exports-json` only on `check`"));
            }
            if request.locked && request.args.len() != 1 && command != "test" {
                return Err(self_decline("accepts `--locked` only on a package route"));
            }
            Ok(())
        }
        [command] | [command, _] if command == "emit" => {
            if !matches!(request.target, Some("rust" | "python")) {
                return Err(self_decline(
                    "does not support this checked emit target in this release",
                ));
            }
            Ok(())
        }
        [command] | [command, _] if command == "build" => {
            if !matches!(
                request.target,
                Some(
                    "default"
                        | "native"
                        | "web"
                        | "web-worker"
                        | "web-app"
                        | "http-service"
                        | "python"
                )
            ) {
                return Err(self_decline(
                    "does not support this checked build target in this release",
                ));
            }
            Ok(())
        }
        [command] if command == "dev" => Ok(()),
        [command] | [command, _] if matches!(command.as_str(), "fmt" | "lsp" | "doc") => {
            if request.profile || request.exports_json || request.locked && command != "doc" {
                return Err(self_decline(
                    "does not accept check-only or unrelated package modifiers on this authoring route",
                ));
            }
            Ok(())
        }
        [compiler, observe] | [compiler, observe, _]
            if compiler == "compiler" && observe == "observe" =>
        {
            if request.profile || request.exports_json {
                return Err(self_decline(
                    "does not accept check-only modifiers on `compiler observe`",
                ));
            }
            Ok(())
        }
        _ => Err(self_decline(
            "does not support this command or modifier in this release",
        )),
    }
}

/// Resolves compiler policy and rejects incompatible command and flag combinations.
pub fn preflight(request: &PreflightRequest<'_>) -> Result<ResolvedCompilerSelection, String> {
    let explicit = matches!(request.intent, CompilerIntent::Explicit(_));
    if explicit && (request.producer || request.self_source) {
        return Err(
            "`--compiler` cannot be combined with bootstrap `--producer` or `--self-source`"
                .to_string(),
        );
    }
    if !is_compiler_bearing(request.args) {
        return match request.intent {
            CompilerIntent::Omitted => Ok(ResolvedCompilerSelection {
                selected_compiler: None,
                selection_origin: None,
            }),
            CompilerIntent::Explicit(selection) => Err(format!(
                "`--compiler {}` applies only to compiler-bearing commands; `compiler preview`, `compiler status`, validation, package management, storage, Lispex, explain, version, license, notice, and help are compiler-neutral",
                selection.selector()
            )),
        };
    }
    let resolved = match request.intent {
        CompilerIntent::Explicit(selection) => ResolvedCompilerSelection {
            selected_compiler: Some(selection),
            selection_origin: Some(SelectionOrigin::Explicit),
        },
        CompilerIntent::Omitted if declared_compatibility_route(request) => {
            ResolvedCompilerSelection {
                selected_compiler: Some(CompilerSelection::Rust),
                selection_origin: Some(SelectionOrigin::Compatibility),
            }
        }
        CompilerIntent::Omitted => ResolvedCompilerSelection {
            selected_compiler: Some(request.product_default),
            selection_origin: Some(SelectionOrigin::CurrentDefault),
        },
    };
    if resolved.selected_compiler == Some(CompilerSelection::SelfHosted) {
        validate_self_selection(request)?;
    }
    Ok(resolved)
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

struct SupportRow {
    command: &'static str,
    rust: &'static str,
    self_hosted: &'static str,
    omitted: &'static str,
    condition: &'static str,
}

const SUPPORT_ROWS: &[SupportRow] = &[
    SupportRow {
        command: "check <entry>",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "current mode; selected entry; profiles and exports",
    },
    SupportRow {
        command: "check <package>",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "current mode; package, locked, profiles, exports",
    },
    SupportRow {
        command: "parse <entry>",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "current mode",
    },
    SupportRow {
        command: "dump-ast <entry>",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "current mode",
    },
    SupportRow {
        command: "run <entry|package>",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "checked current mode",
    },
    SupportRow {
        command: "test <entry|package>",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "current mode",
    },
    SupportRow {
        command: "bench <entry|package>",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "human and JSON current mode",
    },
    SupportRow {
        command: "fmt <entry|package>",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "write and --check; selected parse gate; shared printer",
    },
    SupportRow {
        command: "lsp",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "process-stable current mode selection",
    },
    SupportRow {
        command: "doc <package>",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "selected semantic product; shared renderer",
    },
    SupportRow {
        command: "compiler observe",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "current mode",
    },
    SupportRow {
        command: "emit --target rust",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "checked boxed current mode",
    },
    SupportRow {
        command: "emit --target python",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "checked current-mode target adapter",
    },
    SupportRow {
        command: "build --target native",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "checked boxed current mode; exact compiler provenance",
    },
    SupportRow {
        command: "build --target web",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "checked boxed current mode",
    },
    SupportRow {
        command: "build --target web-worker",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "checked boxed current mode",
    },
    SupportRow {
        command: "build --target web-app",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "checked boxed package route",
    },
    SupportRow {
        command: "build --target http-service",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "checked boxed package route",
    },
    SupportRow {
        command: "build --target python",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "checked current-mode target adapter",
    },
    SupportRow {
        command: "dev --target web-app",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "checked package route",
    },
    SupportRow {
        command: "dev --target http-service",
        rust: "supported",
        self_hosted: "supported",
        omitted: "rust",
        condition: "checked package route",
    },
    SupportRow {
        command: "refactor",
        rust: "supported",
        self_hosted: "declined",
        omitted: "rust",
        condition: "declared Rust compatibility route",
    },
    SupportRow {
        command: "migrate",
        rust: "supported",
        self_hosted: "declined",
        omitted: "rust",
        condition: "declared Rust compatibility route",
    },
    SupportRow {
        command: "check-corpus",
        rust: "repository-only",
        self_hosted: "declined",
        omitted: "rust",
        condition: "repository-only Rust compatibility route",
    },
    SupportRow {
        command: "old language modes",
        rust: "supported",
        self_hosted: "declined",
        omitted: "rust",
        condition: "declared Rust compatibility route",
    },
    SupportRow {
        command: "unchecked,native specialization",
        rust: "supported",
        self_hosted: "declined",
        omitted: "rust",
        condition: "declared Rust compatibility route",
    },
];

/// Renders the compiler-support inventory as canonical JSON for tooling.
pub fn status_json(
    product_version: &str,
    language_mode: &str,
    identity: Result<&InstalledStage2Identity, &str>,
) -> String {
    let mut out = format!(
        "{{\"schema\":{},\"productVersion\":{},\"languageMode\":{},\"selfCompilationProductSchema\":{},\"defaultCompiler\":\"rust\",\"defaultScope\":\"all-compiler-bearing-routes\",\"recoveryCompiler\":\"rust\",\"compatibilityCompiler\":\"rust\",\"bootstrapSeedEngine\":\"rust-stage0\",\"silentFallback\":false,\"selectionOrigins\":[\"explicit\",\"current-default\",\"compatibility\"],\"compilers\":[{{\"selector\":\"rust\",\"producer\":\"rust-stage0\",\"status\":\"supported\"}},",
        json_string(SUPPORT_SCHEMA),
        json_string(product_version),
        json_string(language_mode),
        json_string(SELF_COMPILATION_PRODUCT_SCHEMA),
    );
    match identity {
        Ok(identity) => out.push_str(&format!(
            "{{\"selector\":\"self\",\"producer\":{},\"producerStage\":{},\"status\":\"supported\",\"sourceSetId\":{},\"programImageSha256\":{},\"programImagePayloadSha256\":{},\"exchangeSchema\":{},\"irSchema\":{},\"runtimeTemplate\":{}}}",
            json_string(identity.producer),
            identity.producer_stage,
            json_string(&identity.source_set_id),
            json_string(&identity.program_image_sha256),
            json_string(&identity.program_image_payload_sha256),
            json_string(identity.exchange_schema),
            json_string(identity.ir_schema),
            json_string(identity.runtime_template),
        )),
        Err(reason) => out.push_str(&format!(
            "{{\"selector\":\"self\",\"producer\":\"topaz-stage2\",\"status\":\"unhealthy\",\"reason\":{}}}",
            json_string(reason),
        )),
    }
    out.push_str("],\"routes\":[");
    for (index, row) in SUPPORT_ROWS.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"command\":{},\"rust\":{},\"self\":{},\"omitted\":{},\"condition\":{}}}",
            json_string(row.command),
            json_string(row.rust),
            json_string(row.self_hosted),
            json_string(row.omitted),
            json_string(row.condition),
        ));
    }
    out.push_str("],\"previewProducers\":[\"topaz-stage1\",\"topaz-stage2\"]}\n");
    out
}

/// Renders the compiler-support inventory for an interactive terminal.
pub fn status_human(
    product_version: &str,
    language_mode: &str,
    identity: Result<&InstalledStage2Identity, &str>,
) -> String {
    let self_status = match identity {
        Ok(identity) => format!(
            "Self compiler: self (explicit current-mode {}, stage {})\nSelf compilation product: {SELF_COMPILATION_PRODUCT_SCHEMA}\nSelf program image: {}\n",
            identity.producer, identity.producer_stage, identity.program_image_sha256
        ),
        Err(reason) => format!(
            "Self compiler: self (unhealthy: {reason})\nSelf compilation product: {SELF_COMPILATION_PRODUCT_SCHEMA}\n"
        ),
    };
    format!(
        "Topaz {product_version} compiler support\nLanguage mode: {language_mode}\nDefault compiler: rust\nRecovery compiler: rust\nCompatibility compiler: rust\nBootstrap seed engine: rust-stage0\n{self_status}Silent fallback: disabled\nRun `topaz compiler status --json` for the complete route inventory.\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(args: &[&str], selection: CompilerSelection) -> PreflightRequest<'static> {
        let args = args
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .leak();
        PreflightRequest {
            intent: CompilerIntent::Explicit(selection),
            product_default: CompilerSelection::Rust,
            args,
            self_hosted_default_profile: true,
            locked: false,
            unchecked: false,
            experimental: false,
            profile: false,
            exports_json: false,
            backend_native: false,
            native_report: false,
            producer: false,
            self_source: false,
            target: None,
        }
    }

    #[test]
    fn preflight_is_fail_closed() {
        assert!(
            preflight(&request(
                &["check", "main.tpz"],
                CompilerSelection::SelfHosted
            ))
            .is_ok()
        );
        let old_mode = PreflightRequest {
            self_hosted_default_profile: false,
            ..request(&["check", "main.tpz"], CompilerSelection::SelfHosted)
        };
        let error = preflight(&old_mode).expect_err("old mode declines");
        assert!(error.contains("--compiler rust"));
        assert!(
            preflight(&request(
                &["run", "main.tpz"],
                CompilerSelection::SelfHosted
            ))
            .is_ok()
        );
        let unsupported = request(
            &["refactor", "organize-imports"],
            CompilerSelection::SelfHosted,
        );
        let error = preflight(&unsupported).expect_err("unsupported route declines");
        assert!(error.contains("this release"));
        let neutral = request(&["compiler", "status"], CompilerSelection::Rust);
        assert!(preflight(&neutral).is_err());
    }

    #[test]
    fn resolution_preserves_intent_mode_and_compatibility_origin() {
        let mut current = request(&["check", "main.tpz"], CompilerSelection::Rust);
        current.intent = CompilerIntent::Omitted;
        current.product_default = CompilerSelection::SelfHosted;
        let resolved = preflight(&current).expect("current default resolves");
        assert_eq!(
            resolved,
            ResolvedCompilerSelection {
                selected_compiler: Some(CompilerSelection::SelfHosted),
                selection_origin: Some(SelectionOrigin::CurrentDefault),
            }
        );

        let old_mode = PreflightRequest {
            self_hosted_default_profile: false,
            ..current
        };
        let resolved = preflight(&old_mode).expect("old mode is compatibility");
        assert_eq!(
            resolved,
            ResolvedCompilerSelection {
                selected_compiler: Some(CompilerSelection::Rust),
                selection_origin: Some(SelectionOrigin::Compatibility),
            }
        );

        let mut rust_only = request(&["refactor", "organize-imports"], CompilerSelection::Rust);
        rust_only.intent = CompilerIntent::Omitted;
        rust_only.product_default = CompilerSelection::SelfHosted;
        let resolved = preflight(&rust_only).expect("Rust-only route is compatibility");
        assert_eq!(
            resolved,
            ResolvedCompilerSelection {
                selected_compiler: Some(CompilerSelection::Rust),
                selection_origin: Some(SelectionOrigin::Compatibility),
            }
        );

        let mut neutral = request(&["compiler", "status"], CompilerSelection::Rust);
        neutral.intent = CompilerIntent::Omitted;
        let resolved = preflight(&neutral).expect("neutral omission selects no compiler");
        assert_eq!(
            resolved,
            ResolvedCompilerSelection {
                selected_compiler: None,
                selection_origin: None,
            }
        );
    }

    #[test]
    fn support_v2_records_default_compatibility_and_route_omissions() {
        assert_eq!(SUPPORT_ROWS.len(), 26);
        assert!(SUPPORT_ROWS.iter().all(|row| row.omitted == "rust"));
        let unhealthy = status_json("5.15.3", "topaz-5.15", Err("image mismatch"));
        assert!(unhealthy.contains("\"schema\":\"topaz.compiler-support/v2\""));
        assert!(unhealthy.contains("\"defaultCompiler\":\"rust\""));
        assert!(unhealthy.contains("\"compatibilityCompiler\":\"rust\""));
        assert!(unhealthy.contains("\"status\":\"unhealthy\""));
        assert_eq!(unhealthy.matches("\"omitted\":\"self\"").count(), 0);
        assert_eq!(unhealthy.matches("\"omitted\":\"rust\"").count(), 26);

        let supported = [
            (vec!["check", "main.tpz"], None),
            (vec!["check"], None),
            (vec!["parse", "main.tpz"], None),
            (vec!["dump-ast", "main.tpz"], None),
            (vec!["run", "main.tpz"], None),
            (vec!["test", "main.tpz"], None),
            (vec!["bench", "main.tpz"], None),
            (vec!["fmt", "main.tpz"], None),
            (vec!["lsp"], None),
            (vec!["doc"], None),
            (vec!["compiler", "observe", "main.tpz"], None),
            (vec!["emit", "main.tpz"], Some("rust")),
            (vec!["emit", "main.tpz"], Some("python")),
            (vec!["build", "main.tpz"], Some("native")),
            (vec!["build", "main.tpz"], Some("web")),
            (vec!["build", "main.tpz"], Some("web-worker")),
            (vec!["build"], Some("web-app")),
            (vec!["build"], Some("http-service")),
            (vec!["build", "main.tpz"], Some("python")),
            (vec!["dev"], Some("web-app")),
            (vec!["dev"], Some("http-service")),
        ];
        for (args, target) in supported {
            let mut candidate = request(&args, CompilerSelection::Rust);
            candidate.intent = CompilerIntent::Omitted;
            candidate.product_default = CompilerSelection::Rust;
            candidate.target = target;
            assert_eq!(
                preflight(&candidate).expect("supported omitted route"),
                ResolvedCompilerSelection {
                    selected_compiler: Some(CompilerSelection::Rust),
                    selection_origin: Some(SelectionOrigin::CurrentDefault),
                },
                "{args:?} target {target:?}"
            );
        }

        for args in [
            vec!["refactor", "organize-imports"],
            vec!["migrate", "main.tpz"],
            vec!["check-corpus"],
        ] {
            let mut candidate = request(&args, CompilerSelection::Rust);
            candidate.intent = CompilerIntent::Omitted;
            candidate.product_default = CompilerSelection::SelfHosted;
            assert_eq!(
                preflight(&candidate).expect("compatibility route"),
                ResolvedCompilerSelection {
                    selected_compiler: Some(CompilerSelection::Rust),
                    selection_origin: Some(SelectionOrigin::Compatibility),
                }
            );
        }
    }
}
