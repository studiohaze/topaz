use crate::*;

/// One path and byte slice in the exact embedded self-compiler source set.
pub struct EmbeddedSource {
    pub path: &'static str,
    pub bytes: &'static [u8],
}

#[derive(Debug, Eq, PartialEq)]
/// Per-file byte count and digest in the embedded source manifest.
pub struct EmbeddedSourceManifestEntry {
    pub path: &'static str,
    pub byte_length: usize,
    pub content_sha256: String,
}

#[derive(Debug, Eq, PartialEq)]
/// Canonical inventory and aggregate identity of embedded compiler sources.
pub struct EmbeddedSourceManifest {
    pub schema: &'static str,
    pub source_set_id: String,
    pub files: Vec<EmbeddedSourceManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Lets status and build paths reject a shipped image that disagrees with its sources.
pub struct InstalledStage2Identity {
    pub producer: &'static str,
    pub producer_stage: u8,
    pub source_set_id: String,
    pub program_image_sha256: String,
    pub program_image_payload_sha256: String,
    pub exchange_schema: &'static str,
    pub ir_schema: &'static str,
    pub runtime_template: &'static str,
}

pub const SOURCES: &[EmbeddedSource] = &[
    EmbeddedSource {
        path: "checker.tpz",
        bytes: include_bytes!("../topaz/checker.tpz"),
    },
    EmbeddedSource {
        path: "checker/bodies_captures.tpz",
        bytes: include_bytes!("../topaz/checker/bodies_captures.tpz"),
    },
    EmbeddedSource {
        path: "checker/call_complete.tpz",
        bytes: include_bytes!("../topaz/checker/call_complete.tpz"),
    },
    EmbeddedSource {
        path: "checker/call_facts.tpz",
        bytes: include_bytes!("../topaz/checker/call_facts.tpz"),
    },
    EmbeddedSource {
        path: "checker/call_plan.tpz",
        bytes: include_bytes!("../topaz/checker/call_plan.tpz"),
    },
    EmbeddedSource {
        path: "checker/diagnostics_derives.tpz",
        bytes: include_bytes!("../topaz/checker/diagnostics_derives.tpz"),
    },
    EmbeddedSource {
        path: "checker/expression_control.tpz",
        bytes: include_bytes!("../topaz/checker/expression_control.tpz"),
    },
    EmbeddedSource {
        path: "checker/expression_core.tpz",
        bytes: include_bytes!("../topaz/checker/expression_core.tpz"),
    },
    EmbeddedSource {
        path: "checker/members_capabilities.tpz",
        bytes: include_bytes!("../topaz/checker/members_capabilities.tpz"),
    },
    EmbeddedSource {
        path: "checker/model.tpz",
        bytes: include_bytes!("../topaz/checker/model.tpz"),
    },
    EmbeddedSource {
        path: "checker/nominals.tpz",
        bytes: include_bytes!("../topaz/checker/nominals.tpz"),
    },
    EmbeddedSource {
        path: "checker/patterns.tpz",
        bytes: include_bytes!("../topaz/checker/patterns.tpz"),
    },
    EmbeddedSource {
        path: "checker/protocols_methods.tpz",
        bytes: include_bytes!("../topaz/checker/protocols_methods.tpz"),
    },
    EmbeddedSource {
        path: "checker/statements.tpz",
        bytes: include_bytes!("../topaz/checker/statements.tpz"),
    },
    EmbeddedSource {
        path: "checker/syntax_index.tpz",
        bytes: include_bytes!("../topaz/checker/syntax_index.tpz"),
    },
    EmbeddedSource {
        path: "checker/type_algebra.tpz",
        bytes: include_bytes!("../topaz/checker/type_algebra.tpz"),
    },
    EmbeddedSource {
        path: "checker_types.tpz",
        bytes: include_bytes!("../topaz/checker_types.tpz"),
    },
    EmbeddedSource {
        path: "emitter.tpz",
        bytes: include_bytes!("../topaz/emitter.tpz"),
    },
    EmbeddedSource {
        path: "layout.tpz",
        bytes: include_bytes!("../topaz/layout.tpz"),
    },
    EmbeddedSource {
        path: "lowering.tpz",
        bytes: include_bytes!("../topaz/lowering.tpz"),
    },
    EmbeddedSource {
        path: "parser.tpz",
        bytes: include_bytes!("../topaz/parser.tpz"),
    },
    EmbeddedSource {
        path: "parser/core.tpz",
        bytes: include_bytes!("../topaz/parser/core.tpz"),
    },
    EmbeddedSource {
        path: "parser/declarations_modules.tpz",
        bytes: include_bytes!("../topaz/parser/declarations_modules.tpz"),
    },
    EmbeddedSource {
        path: "parser/expressions.tpz",
        bytes: include_bytes!("../topaz/parser/expressions.tpz"),
    },
    EmbeddedSource {
        path: "parser/types_patterns.tpz",
        bytes: include_bytes!("../topaz/parser/types_patterns.tpz"),
    },
    EmbeddedSource {
        path: "profile.tpz",
        bytes: include_bytes!("../topaz/profile.tpz"),
    },
    EmbeddedSource {
        path: "raw.tpz",
        bytes: include_bytes!("../topaz/raw.tpz"),
    },
    EmbeddedSource {
        path: "resolver.tpz",
        bytes: include_bytes!("../topaz/resolver.tpz"),
    },
    EmbeddedSource {
        path: "resolver/graph.tpz",
        bytes: include_bytes!("../topaz/resolver/graph.tpz"),
    },
    EmbeddedSource {
        path: "resolver/model_facts.tpz",
        bytes: include_bytes!("../topaz/resolver/model_facts.tpz"),
    },
    EmbeddedSource {
        path: "resolver/modules_tables.tpz",
        bytes: include_bytes!("../topaz/resolver/modules_tables.tpz"),
    },
    EmbeddedSource {
        path: "resolver/scopes_walk.tpz",
        bytes: include_bytes!("../topaz/resolver/scopes_walk.tpz"),
    },
    EmbeddedSource {
        path: "resolver/validation_init.tpz",
        bytes: include_bytes!("../topaz/resolver/validation_init.tpz"),
    },
    EmbeddedSource {
        path: "src/main.tpz",
        bytes: include_bytes!("../topaz/src/main.tpz"),
    },
    EmbeddedSource {
        path: "stage1_types.tpz",
        bytes: include_bytes!("../topaz/stage1_types.tpz"),
    },
    EmbeddedSource {
        path: "types.tpz",
        bytes: include_bytes!("../topaz/types.tpz"),
    },
    EmbeddedSource {
        path: "unicode.tpz",
        bytes: include_bytes!("../topaz/unicode.tpz"),
    },
    EmbeddedSource {
        path: "unicode_tables.tpz",
        bytes: include_bytes!("../topaz/unicode_tables.tpz"),
    },
];

/// Derives the framed aggregate identity of the embedded source inventory.
pub fn source_set_id() -> String {
    let mut framed = Vec::new();
    for source in SOURCES {
        framed.extend_from_slice(source.path.as_bytes());
        framed.push(0);
        framed.extend_from_slice(source.bytes.len().to_string().as_bytes());
        framed.push(0);
        let digest = topaz_value::value::sha256(source.bytes);
        let mut hex = String::new();
        topaz_value::bytes_to_hex_into(&mut hex, &digest);
        framed.extend_from_slice(hex.as_bytes());
        framed.push(b'\n');
    }
    let digest = topaz_value::value::sha256(&framed);
    let mut identity = String::from("sha256:");
    topaz_value::bytes_to_hex_into(&mut identity, &digest);
    identity
}

/// Projects the embedded source inventory with per-file content identities.
pub fn source_manifest() -> EmbeddedSourceManifest {
    EmbeddedSourceManifest {
        schema: "topaz.compiler.embedded-source-manifest/v1",
        source_set_id: source_set_id(),
        files: SOURCES
            .iter()
            .map(|source| {
                let digest = topaz_value::value::sha256(source.bytes);
                let mut content_sha256 = String::from("sha256:");
                topaz_value::bytes_to_hex_into(&mut content_sha256, &digest);
                EmbeddedSourceManifestEntry {
                    path: source.path,
                    byte_length: source.bytes.len(),
                    content_sha256,
                }
            })
            .collect(),
    }
}

/// Validate and report the exact stage-neutral image used by the installed
/// Stage 2 route. Payload validation also parses the compact program, so
/// status reporting fails instead of advertising a corrupt self compiler.
pub fn installed_stage2_identity() -> Result<InstalledStage2Identity, String> {
    let (generated_source_set, runtime_template, ir_schema) =
        topaz_stage1_runtime::embedded_compiler_identity();
    let prepared = topaz_stage1_runtime::prepared_embedded_stage2_identity()?;
    if prepared.source_set_id != generated_source_set
        || prepared.runtime_template != runtime_template
        || prepared.ir_schema != ir_schema
        || prepared.rust_toolchain != env!("CARGO_PKG_RUST_VERSION")
        || runtime_template != FIXED_POINT_RUNTIME_TEMPLATE
        || ir_schema != STAGE1_IR_SCHEMA
    {
        return Err("embedded C2 schema or runtime-template identity drifted".to_string());
    }
    Ok(InstalledStage2Identity {
        producer: CompilerProducer::Stage2.identity(),
        producer_stage: 2,
        source_set_id: generated_source_set.to_string(),
        program_image_sha256: prepared.program_image_sha256,
        program_image_payload_sha256: prepared.program_image_payload_sha256,
        exchange_schema: STAGE1_EXCHANGE_SCHEMA,
        ir_schema: STAGE1_IR_SCHEMA,
        runtime_template: FIXED_POINT_RUNTIME_TEMPLATE,
    })
}

/// Fact source exposing only the checked-in embedded compiler package.
pub struct EmbeddedCompilerSourceHost;

/// Adds every embedded compiler source as an explicit root-mount fact.
pub fn supply_embedded_compiler_source_facts(
    request: &mut topaz_kernel::KernelRequest,
) -> Result<(), topaz_kernel::FactError> {
    for source in SOURCES {
        request.supply_fact(
            topaz_kernel::HostQuery::ReadSource {
                mount_id: "root".to_string(),
                logical_path: source.path.to_string(),
            },
            topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Present(
                std::str::from_utf8(source.bytes)
                    .expect("embedded Topaz compiler source must remain UTF-8")
                    .to_string(),
            )),
        )?;
    }
    Ok(())
}

impl topaz_kernel::HostFactSource for EmbeddedCompilerSourceHost {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(
                    SOURCES
                        .iter()
                        .find(|source| source.path == logical_path)
                        .map(|source| {
                            std::str::from_utf8(source.bytes)
                                .map(str::to_string)
                                .map(topaz_kernel::SourceFact::Present)
                                .unwrap_or_else(|error| topaz_kernel::SourceFact::Unreadable {
                                    reason_code: error.to_string(),
                                })
                        })
                        .unwrap_or(topaz_kernel::SourceFact::Missing),
                )
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                let prefix = if logical_path.is_empty() {
                    String::new()
                } else {
                    format!("{logical_path}/")
                };
                let mut entries = std::collections::BTreeMap::new();
                for source in SOURCES {
                    let Some(rest) = source.path.strip_prefix(&prefix) else {
                        continue;
                    };
                    let (name, kind) = match rest.split_once('/') {
                        Some((directory, _)) => (
                            directory.to_string(),
                            topaz_kernel::DirectoryEntryKind::Directory,
                        ),
                        None => (rest.to_string(), topaz_kernel::DirectoryEntryKind::File),
                    };
                    entries.insert(name, kind);
                }
                topaz_kernel::HostFact::Directory(if entries.is_empty() {
                    topaz_kernel::DirectoryFact::Missing
                } else {
                    topaz_kernel::DirectoryFact::Present(
                        entries
                            .into_iter()
                            .map(|(name, kind)| topaz_kernel::DirectoryEntry { name, kind })
                            .collect(),
                    )
                })
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: format!("stage1-compiler:{logical_path}"),
                })
            }
        }
    }
}

/// Resolves the embedded compiler package from its in-memory source inventory.
pub fn resolve_embedded() -> Result<ResolveOutput, String> {
    let mut provider = InMemoryProvider::new();
    for source in SOURCES {
        let text = std::str::from_utf8(source.bytes)
            .map_err(|error| format!("embedded `{}` is not UTF-8: {error}", source.path))?;
        provider.add_file(source.path, text);
    }
    let unit = resolve(&provider, "src/main.tpz", Some(""));
    if unit.diagnostics.is_empty() {
        Ok(unit)
    } else {
        Err(format!(
            "embedded front end did not resolve: {:?}",
            unit.diagnostics
        ))
    }
}
