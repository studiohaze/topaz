use crate::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Derivation stage whose compiler image produces the exchange response.
pub enum CompilerProducer {
    Stage1,
    /// Compatibility routing identity over the shared stage-neutral program
    /// image. The identity comes from the derivation edge and is not evidence
    /// of a separately derived compiler generation.
    Stage2,
}

impl CompilerProducer {
    pub const fn identity(self) -> &'static str {
        match self {
            Self::Stage1 => "topaz-stage1",
            Self::Stage2 => "topaz-stage2",
        }
    }

    pub const fn stage(self) -> i64 {
        match self {
            Self::Stage1 => 1,
            Self::Stage2 => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Named self-compilation policy selected for an otherwise identical request.
pub enum CompilationProfile {
    None,
    AgentPack,
    TestProfile,
    Bootstrap,
}

impl CompilationProfile {
    pub const fn identity(self) -> &'static str {
        match self {
            Self::None => "",
            Self::AgentPack => "agent-pack",
            Self::TestProfile => "test-profile",
            Self::Bootstrap => "bootstrap",
        }
    }
}

pub(crate) fn json_quote(value: &str) -> Result<String, String> {
    json_stringify(&Value::str(value), true).map_err(|error| error.to_string())
}

pub(crate) fn push_json_string_array(
    output: &mut String,
    values: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    output.push('[');
    for (index, value) in values.into_iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push_str(&json_quote(&value)?);
    }
    output.push(']');
    Ok(())
}

pub(crate) fn encode_preview_request(
    request: &topaz_kernel::KernelRequest,
    terminal: &str,
) -> Result<Vec<u8>, String> {
    use topaz_kernel::{
        BuildRole, ContainmentFact, DirectoryEntryKind, DirectoryFact, HostFact, HostQuery,
        SourceFact,
    };
    let root = request
        .mounts()
        .first()
        .ok_or_else(|| "front-end preview request has no root mount".to_string())?;
    let mut output = String::from("{\"schema\":");
    output.push_str(&json_quote(EXCHANGE_SCHEMA)?);
    output.push_str(",\"terminal\":");
    output.push_str(&json_quote(terminal)?);
    output.push_str(",\"entry\":");
    output.push_str(&json_quote(request.entry())?);
    output.push_str(",\"root\":");
    output.push_str(&json_quote(&root.logical_root)?);
    output.push_str(",\"source\":\"\",\"sourceId\":");
    output.push_str(&json_quote(request.entry())?);
    output.push_str(",\"facts\":[");
    for (ordinal, (query, fact)) in request.facts().iter().enumerate() {
        if ordinal > 0 {
            output.push(',');
        }
        let (kind, mount_id, logical_path) = match query {
            HostQuery::ReadSource {
                mount_id,
                logical_path,
            } => ("read-source", mount_id, logical_path),
            HostQuery::ListDirectory {
                mount_id,
                logical_path,
            } => ("list-directory", mount_id, logical_path),
            HostQuery::PhysicalContainment {
                mount_id,
                logical_path,
            } => ("physical-containment", mount_id, logical_path),
        };
        let (status, source, entries, alias_class, reason_code) = match fact {
            HostFact::Source(SourceFact::Present(source)) => {
                ("present", source.as_str(), None, "", "")
            }
            HostFact::Source(SourceFact::Missing) => ("missing", "", None, "", ""),
            HostFact::Source(SourceFact::Unreadable { reason_code }) => {
                ("unreadable", "", None, "", reason_code.as_str())
            }
            HostFact::Source(SourceFact::InvalidUtf8) => ("invalid-utf8", "", None, "", ""),
            HostFact::Directory(DirectoryFact::Present(entries)) => {
                ("present", "", Some(entries.as_slice()), "", "")
            }
            HostFact::Directory(DirectoryFact::Missing) => ("missing", "", None, "", ""),
            HostFact::Directory(DirectoryFact::Unreadable { reason_code }) => {
                ("unreadable", "", None, "", reason_code.as_str())
            }
            HostFact::Containment(ContainmentFact::Inside { alias_class }) => {
                ("inside", "", None, alias_class.as_str(), "")
            }
            HostFact::Containment(ContainmentFact::Outside) => ("outside", "", None, "", ""),
            HostFact::Containment(ContainmentFact::Missing) => ("missing", "", None, "", ""),
            HostFact::Containment(ContainmentFact::Unresolved) => ("unresolved", "", None, "", ""),
        };
        output.push_str("{\"kind\":");
        output.push_str(&json_quote(kind)?);
        output.push_str(",\"mountId\":");
        output.push_str(&json_quote(mount_id)?);
        output.push_str(",\"logicalPath\":");
        output.push_str(&json_quote(logical_path)?);
        output.push_str(",\"status\":");
        output.push_str(&json_quote(status)?);
        output.push_str(",\"source\":");
        output.push_str(&json_quote(source)?);
        output.push_str(",\"entries\":[");
        if let Some(entries) = entries {
            for (index, entry) in entries.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str("{\"name\":");
                output.push_str(&json_quote(&entry.name)?);
                output.push_str(",\"kind\":");
                output.push_str(&json_quote(match entry.kind {
                    DirectoryEntryKind::File => "file",
                    DirectoryEntryKind::Directory => "directory",
                })?);
                output.push('}');
            }
        }
        output.push_str("],\"aliasClass\":");
        output.push_str(&json_quote(alias_class)?);
        output.push_str(",\"reasonCode\":");
        output.push_str(&json_quote(reason_code)?);
        output.push('}');
    }
    output.push_str("],\"package\":{\"buildRole\":");
    output.push_str(&json_quote(match request.package().build_role {
        BuildRole::Standalone => "standalone",
        BuildRole::Package => "package",
    })?);
    output.push_str(",\"externModules\":");
    push_json_string_array(
        &mut output,
        request.package().extern_modules.iter().cloned(),
    )?;
    output.push_str(",\"externReplayModules\":");
    push_json_string_array(
        &mut output,
        request.package().extern_replay_errors.keys().cloned(),
    )?;
    output.push_str(",\"externReplayErrors\":");
    push_json_string_array(
        &mut output,
        request.package().extern_replay_errors.values().cloned(),
    )?;
    output.push_str(",\"generatedStdModules\":[");
    for (index, (identity, module)) in request.package().generated_std_modules.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push_str("{\"identity\":");
        output.push_str(&json_quote(identity)?);
        output.push_str(",\"path\":");
        output.push_str(&json_quote(&module.path)?);
        output.push_str(",\"source\":");
        output.push_str(&json_quote(&module.source)?);
        output.push('}');
    }
    output.push(']');
    output.push_str("},\"maxAstNodes\":");
    output.push_str(&request.budgets().max_ast_nodes.to_string());
    output.push_str(",\"maxAstDepth\":");
    output.push_str(&MAX_AST_DEPTH.to_string());
    output.push('}');
    Ok(output.into_bytes())
}

/// Encodes a fact-complete compiler request with explicit producer and profile identity.
pub fn encode_compiler_request_with_profile(
    request: &topaz_kernel::KernelRequest,
    producer: CompilerProducer,
    profile: CompilationProfile,
) -> Result<Vec<u8>, String> {
    let terminal = match request.terminal_phase() {
        topaz_kernel::TerminalPhase::Lowered => "lowered",
        topaz_kernel::TerminalPhase::RustSource => "rust-source",
        _ => return Err("Stage 1 requires the lowered or rust-source terminal".to_string()),
    };
    let encoded = encode_preview_request(request, terminal)?;
    let mut output = String::from_utf8(encoded)
        .map_err(|error| format!("internal Stage 1 request is not UTF-8: {error}"))?;
    let schema = json_quote(EXCHANGE_SCHEMA)?;
    let stage1_schema = json_quote(STAGE1_EXCHANGE_SCHEMA)?;
    output = output.replacen(&schema, &stage1_schema, 1);
    if output.pop() != Some('}') {
        return Err("internal Stage 1 request envelope is not an object".to_string());
    }
    let budgets = request.budgets();
    output.push_str(",\"producer\":");
    output.push_str(&json_quote(producer.identity())?);
    output.push_str(",\"compilerSourceSetId\":");
    output.push_str(&json_quote(&source_set_id())?);
    output.push_str(",\"languageMode\":");
    output.push_str(&json_quote(&format!(
        "topaz-{}",
        request.language_version().as_str()
    ))?);
    output.push_str(",\"profile\":");
    output.push_str(&json_quote(profile.identity())?);
    for (field, value) in [
        ("maxSourceFacts", budgets.max_source_facts),
        ("maxTotalSourceBytes", budgets.max_total_source_bytes),
        ("maxRawTokens", budgets.max_raw_tokens),
        ("maxLayoutTokens", budgets.max_layout_tokens),
        ("maxHirNodes", budgets.max_hir_nodes),
        ("maxLoweredNodes", budgets.max_lowered_nodes),
        ("maxDiagnostics", budgets.max_diagnostics),
        ("maxGeneratedRustBytes", budgets.max_generated_rust_bytes),
        (
            "maxFactRounds",
            budgets.max_source_facts.saturating_mul(3).saturating_add(4),
        ),
    ] {
        output.push(',');
        output.push('"');
        output.push_str(field);
        output.push_str("\":");
        output.push_str(&value.to_string());
    }
    output.push('}');
    Ok(output.into_bytes())
}

/// Encodes an unprofiled request for the selected compiler producer.
pub fn encode_compiler_request(
    request: &topaz_kernel::KernelRequest,
    producer: CompilerProducer,
) -> Result<Vec<u8>, String> {
    encode_compiler_request_with_profile(request, producer, CompilationProfile::None)
}

/// Encodes the compatibility Stage 1 compiler request envelope.
pub fn encode_stage1_request(request: &topaz_kernel::KernelRequest) -> Result<Vec<u8>, String> {
    encode_compiler_request(request, CompilerProducer::Stage1)
}

/// Encodes the Stage 2 compiler request envelope.
pub fn encode_stage2_request(request: &topaz_kernel::KernelRequest) -> Result<Vec<u8>, String> {
    encode_compiler_request(request, CompilerProducer::Stage2)
}
