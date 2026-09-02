use super::*;

#[test]
fn resolver_fact_replay_is_order_independent_and_has_no_hidden_fallback() {
    let host = ResolverFixtureHost::new();
    let first = preview_resolved(&host, resolver_request()).expect("initial resolution");
    assert!(host.responses.get() > 0);
    assert_eq!(
        first
            .modules
            .iter()
            .map(|module| module.identity.as_str())
            .collect::<Vec<_>>(),
        ["lib", "main"]
    );
    let first_bundle = resolved_preview_bundle(&first);
    first_bundle.validate().expect("valid initial observation");

    let mut replay = resolver_request();
    for (query, fact) in first.request.facts().iter().rev() {
        replay
            .supply_fact(query.clone(), fact.clone())
            .expect("unique replay fact");
    }
    let second = preview_resolved(&NoFactHost, replay).expect("fact-only replay");
    let second_bundle = resolved_preview_bundle(&second);
    second_bundle.validate().expect("valid replay observation");
    assert_eq!(first_bundle, second_bundle);
}

#[test]
fn resolver_admission_imported_protocol_surface_is_valid_in_stage0_and_self_host() {
    let self_preview = preview_resolved(&ResolverFixtureHost::new(), resolver_request())
        .expect("self-host imported protocol resolution");
    assert!(
        self_preview.diagnostics.is_empty(),
        "self-host must admit an imported protocol declaration: {:?}",
        self_preview.diagnostics
    );
    assert!(self_preview.declarations.iter().any(|declaration| {
        declaration.name == "Measure" && declaration.declaration_kind == "protocol"
    }));

    let mut provider = topaz_resolve::InMemoryProvider::new();
    provider.add_file("root/main.tpz", RESOLVER_ENTRY_SOURCE);
    provider.add_file("root/lib.tpz", RESOLVER_LIBRARY_SOURCE);
    let stage0 = topaz_resolve::resolve_with_version(
        &provider,
        "root/main.tpz",
        Some("root"),
        LangVersion::CURRENT,
    );
    assert!(
        stage0.diagnostics.is_empty(),
        "Stage 0 must admit an imported protocol declaration: {:?}",
        stage0.diagnostics
    );
}

#[test]
fn resolver_admission_outside_entry_stops_before_import_discovery_like_stage0() {
    let self_preview = preview_resolved(&OutsideEntryResolverHost, resolver_request())
        .expect("self-host outside-entry rejection");

    let mut provider = topaz_resolve::InMemoryProvider::new();
    provider.add_file("outside/main.tpz", RESOLVER_ENTRY_SOURCE);
    provider.add_link("root/main.tpz", "outside/main.tpz");
    let stage0 = topaz_resolve::resolve_with_version(
        &provider,
        "root/main.tpz",
        Some("root"),
        LangVersion::CURRENT,
    );
    assert_eq!(
        provider.reads(),
        std::collections::BTreeSet::from(["root/main.tpz".to_string()]),
        "Stage 0 must not read imports after entry containment rejection"
    );
    let stage0_diagnostics = stage0
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        self_diagnostics,
        vec![(
            "TPZ3005",
            "the entry `root/main.tpz` resolves outside the root (symlink/alias containment)",
        )],
    );
    assert!(self_preview.modules.is_empty());
}

#[test]
fn resolver_discovery_keeps_lone_cr_inside_line_comment_like_stage0() {
    let self_preview = preview_resolved(
        &EntryOnlyResolverHost {
            source: LONE_CR_COMMENTED_IMPORT_SOURCE,
            alias_class: "lone-cr-commented-import",
        },
        resolver_request(),
    )
    .expect("self-host lone-CR line-comment resolution");

    let mut provider = topaz_resolve::InMemoryProvider::new();
    provider.add_file("root/main.tpz", LONE_CR_COMMENTED_IMPORT_SOURCE);
    provider.add_file("root/lib.tpz", "export const value = 42\n");
    let stage0 = topaz_resolve::resolve_with_version(
        &provider,
        "root/main.tpz",
        Some("root"),
        LangVersion::CURRENT,
    );

    assert_eq!(
        provider.reads(),
        std::collections::BTreeSet::from(["root/main.tpz".to_string()]),
        "Stage 0 must keep an apparent import after lone CR inside the line comment",
    );
    assert!(stage0.diagnostics.is_empty(), "{:?}", stage0.diagnostics);
    assert!(
        self_preview.diagnostics.is_empty(),
        "{:?}",
        self_preview.diagnostics
    );
}

#[test]
fn resolver_discovery_does_not_cross_an_import_head_separator() {
    for (separator, source) in SEPARATED_IMPORT_HEAD_SOURCES {
        let self_preview = preview_resolved(
            &EntryOnlyResolverHost {
                source,
                alias_class: separator,
            },
            resolver_request(),
        )
        .expect("self-host separated import-head resolution");

        let mut provider = topaz_resolve::InMemoryProvider::new();
        provider.add_file("root/main.tpz", source);
        provider.add_file("root/lib.tpz", "export const value = 42\n");
        let stage0 = topaz_resolve::resolve_with_version(
            &provider,
            "root/main.tpz",
            Some("root"),
            LangVersion::CURRENT,
        );

        assert_eq!(
            provider.reads(),
            std::collections::BTreeSet::from(["root/main.tpz".to_string()]),
            "Stage 0 read across the {separator} separator after an import head",
        );
        assert!(stage0.import_edges.is_empty(), "{separator}");
        assert!(self_preview.edges.is_empty(), "{separator}");
    }
}

#[test]
fn resolver_discovery_preserves_dotted_path_trivia_like_stage0() {
    for (trivia, source) in DOTTED_IMPORT_TRIVIA_SOURCES {
        let self_preview = preview_resolved(
            &DottedImportTriviaResolverHost {
                source,
                alias_class: trivia,
            },
            resolver_request(),
        )
        .expect("self-host dotted import-trivia resolution");

        let mut provider = topaz_resolve::InMemoryProvider::new();
        provider.add_file("root/main.tpz", source);
        provider.add_file("root/lib/sub.tpz", "export const value = 42\n");
        let stage0 = topaz_resolve::resolve_with_version(
            &provider,
            "root/main.tpz",
            Some("root"),
            LangVersion::CURRENT,
        );

        assert_eq!(
            provider.reads(),
            std::collections::BTreeSet::from([
                "root/lib/sub.tpz".to_string(),
                "root/main.tpz".to_string(),
            ]),
            "Stage 0 dotted import reads drifted for {trivia}",
        );
        assert_eq!(
            stage0.import_edges,
            vec![("main".to_string(), "lib.sub".to_string())],
            "{trivia}",
        );
        assert_eq!(
            self_preview
                .edges
                .iter()
                .map(|edge| (edge.from.as_str(), edge.to.as_str()))
                .collect::<Vec<_>>(),
            [("main", "lib.sub")],
            "{trivia}",
        );
    }
}

#[test]
fn resolver_discovery_requires_identifier_tokens_for_import_paths_like_stage0() {
    for (form, source) in NON_IDENTIFIER_IMPORT_PATH_SOURCES {
        let self_preview = preview_resolved(
            &EntryOnlyResolverHost {
                source,
                alias_class: form,
            },
            resolver_request(),
        )
        .expect("self-host non-identifier import-path resolution");

        let mut provider = topaz_resolve::InMemoryProvider::new();
        provider.add_file("root/main.tpz", source);
        let stage0 = topaz_resolve::resolve_with_version(
            &provider,
            "root/main.tpz",
            Some("root"),
            LangVersion::CURRENT,
        );

        assert_eq!(
            provider.reads(),
            std::collections::BTreeSet::from(["root/main.tpz".to_string()]),
            "Stage 0 read a non-identifier import path for {form}",
        );
        assert!(stage0.import_edges.is_empty(), "{form}");
        assert!(self_preview.edges.is_empty(), "{form}");
    }
}

#[test]
fn resolver_discovery_ignores_comment_braces_between_imports_like_stage0() {
    for (comment, source) in COMMENT_BRACED_IMPORT_PROLOGUE_SOURCES {
        let self_preview = preview_resolved(
            &CommentBracedImportPrologueResolverHost {
                source,
                alias_class: comment,
            },
            resolver_request(),
        )
        .expect("self-host comment-braced import-prologue resolution");

        let mut provider = topaz_resolve::InMemoryProvider::new();
        provider.add_file("root/main.tpz", source);
        provider.add_file("root/lib.tpz", "export const value = 42\n");
        provider.add_file("root/other.tpz", "export const value = 42\n");
        let stage0 = topaz_resolve::resolve_with_version(
            &provider,
            "root/main.tpz",
            Some("root"),
            LangVersion::CURRENT,
        );

        assert_eq!(self_preview.rounds, 5, "{comment}");
        assert_eq!(
            provider.reads(),
            std::collections::BTreeSet::from([
                "root/lib.tpz".to_string(),
                "root/main.tpz".to_string(),
                "root/other.tpz".to_string(),
            ]),
            "Stage 0 import-prologue reads drifted for {comment}",
        );
        assert_eq!(
            stage0.import_edges,
            vec![
                ("main".to_string(), "lib".to_string()),
                ("main".to_string(), "other".to_string()),
            ],
            "{comment}",
        );
        assert_eq!(
            self_preview
                .edges
                .iter()
                .map(|edge| (edge.from.as_str(), edge.to.as_str()))
                .collect::<Vec<_>>(),
            [("main", "lib"), ("main", "other")],
            "{comment}",
        );
    }
}

#[test]
fn resolver_admission_outside_import_stops_before_path_and_source_reads_like_stage0() {
    let self_preview = preview_resolved(&OutsideImportResolverHost, resolver_request())
        .expect("self-host outside-import rejection");

    let mut provider = topaz_resolve::InMemoryProvider::new();
    provider.add_file("root/main.tpz", RESOLVER_ENTRY_SOURCE);
    provider.add_file("outside/lib.tpz", RESOLVER_LIBRARY_SOURCE);
    provider.add_link("root/lib.tpz", "outside/lib.tpz");
    let stage0 = topaz_resolve::resolve_with_version(
        &provider,
        "root/main.tpz",
        Some("root"),
        LangVersion::CURRENT,
    );
    assert_eq!(
        provider.reads(),
        std::collections::BTreeSet::from(["root/main.tpz".to_string()]),
        "Stage 0 must not read an import after containment rejection"
    );
    let stage0_diagnostics = stage0
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        self_diagnostics,
        vec![(
            "TPZ3005",
            "`lib` resolves outside the root (symlink/alias containment)",
        )],
    );
}

#[test]
fn resolver_admission_missing_segment_stops_before_deeper_path_and_source_reads() {
    let self_preview = preview_resolved(&MissingFirstSegmentResolverHost, resolver_request())
        .expect("self-host missing-segment rejection");

    let mut provider = topaz_resolve::InMemoryProvider::new();
    provider.add_file("root/main.tpz", NESTED_MISSING_ENTRY_SOURCE);
    let stage0 = topaz_resolve::resolve_with_version(
        &provider,
        "root/main.tpz",
        Some("root"),
        LangVersion::CURRENT,
    );
    assert_eq!(
        provider.reads(),
        std::collections::BTreeSet::from(["root/main.tpz".to_string()]),
        "Stage 0 must not read a module whose first path segment is missing"
    );
    let stage0_diagnostics = stage0
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        self_diagnostics,
        vec![(
            "TPZ3001",
            "no module file for `missing.lib` (expected `root/missing/lib.tpz` by exact scalars)",
        )],
    );
}

#[test]
fn resolver_unreadable_directory_diagnostic_matches_stage0() {
    let request = resolver_request();
    let self_preview = preview_resolved(&UnreadableDirectoryResolverHost, request.clone())
        .expect("self-host unreadable-directory resolution");
    let stage0 = stage0_resolved_unit(&UnreadableDirectoryResolverHost, request);
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        stage0_diagnostics,
        vec![(
            "TPZ3003",
            "cannot inspect module path for `lib`: permission-denied",
        )],
    );
}

#[test]
fn resolver_unavailable_source_diagnostics_match_stage0() {
    for (entry, kind, expected_message) in [
        (
            true,
            UnavailableSourceKind::Unreadable,
            "cannot load entry `root/main.tpz`: permission-denied",
        ),
        (
            true,
            UnavailableSourceKind::InvalidUtf8,
            "cannot load entry `root/main.tpz`: source is not valid UTF-8",
        ),
        (
            false,
            UnavailableSourceKind::Unreadable,
            "cannot load module `lib`: permission-denied",
        ),
        (
            false,
            UnavailableSourceKind::InvalidUtf8,
            "cannot load module `lib`: source is not valid UTF-8",
        ),
    ] {
        let host = UnavailableSourceResolverHost { entry, kind };
        let request = resolver_request();
        let self_preview = preview_resolved(&host, request.clone())
            .expect("self-host unavailable-source resolution");
        let stage0 = stage0_resolved_unit(&host, request);
        let self_diagnostics = self_preview
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
            .collect::<Vec<_>>();
        let stage0_diagnostics = stage0
            .resolved
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(self_diagnostics, stage0_diagnostics);
        assert_eq!(stage0_diagnostics, vec![("TPZ3003", expected_message)]);
    }
}

#[test]
fn resolver_repeated_missing_target_diagnostic_matches_stage0() {
    let host = ResolverSourcesHost {
        a_source: "import b\nimport c\n",
        b_source: Some("import missing\nexport const bValue = 1\n"),
        c_source: Some("import missing\nexport const cValue = 1\n"),
        alias_class: "repeated-missing-target",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host repeated missing-target resolution");
    let stage0 = stage0_resolved_unit(&host, request);
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        stage0_diagnostics,
        vec![(
            "TPZ3001",
            "no module file for `missing` (expected `root/missing.tpz` by exact scalars)",
        )],
    );
}

#[test]
fn resolver_undeclared_extern_namespace_diagnostic_matches_stage0() {
    let mut package = topaz_kernel::PackageFacts::standalone();
    package.extern_modules.insert("host.math".to_string());
    let request = topaz_kernel::KernelRequest::checked(
        "root/main.tpz",
        Some("root"),
        LangVersion::CURRENT,
        package,
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&UndeclaredExternResolverHost, request.clone())
        .expect("self-host undeclared extern resolution");
    let stage0 = topaz_kernel::drive_checked(&UndeclaredExternResolverHost, request);
    let stage0_unit = match stage0.outcome {
        topaz_kernel::KernelOutcome::Rejected(unit) => unit,
        topaz_kernel::KernelOutcome::Completed(_) => {
            panic!("Stage 0 admitted an undeclared extern sibling")
        }
        topaz_kernel::KernelOutcome::NeedHostFacts(queries) => {
            panic!("Stage 0 left undeclared extern facts pending: {queries:?}")
        }
        topaz_kernel::KernelOutcome::Declined { reason } => {
            panic!("Stage 0 declined undeclared extern fixture: {reason}")
        }
        topaz_kernel::KernelOutcome::ResourceLimit(limit) => {
            panic!("Stage 0 exhausted undeclared extern fixture resource: {limit:?}")
        }
        topaz_kernel::KernelOutcome::CompilerFault { message } => {
            panic!("Stage 0 undeclared extern compiler fault: {message}")
        }
    };
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0_unit
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        stage0_diagnostics,
        vec![(
            topaz_diag::extern_codes::DECL,
            "extern module `host.video` is not declared in topaz.toml",
        )],
    );
}

#[test]
fn checker_invalid_extern_replay_diagnostic_matches_stage0() {
    let mut package = topaz_kernel::PackageFacts::standalone();
    package.extern_modules.insert("host.math".to_string());
    package.extern_replay_errors.insert(
        "host.math".to_string(),
        "fixture row has an invalid result shape".to_string(),
    );
    let request =
        topaz_kernel::KernelRequest::checked("main.tpz", Some(""), LangVersion::CURRENT, package)
            .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    let self_preview = preview_typed(&InvalidExternReplayResolverHost, request.clone())
        .expect("self-host invalid extern replay check");
    let stage0 = topaz_kernel::drive_checked(&InvalidExternReplayResolverHost, request);
    let stage0_unit = match stage0.outcome {
        topaz_kernel::KernelOutcome::Rejected(unit) => unit,
        topaz_kernel::KernelOutcome::Completed(_) => {
            panic!("Stage 0 admitted an invalid extern replay binding")
        }
        topaz_kernel::KernelOutcome::NeedHostFacts(queries) => {
            panic!("Stage 0 left invalid extern replay facts pending: {queries:?}")
        }
        topaz_kernel::KernelOutcome::Declined { reason } => {
            panic!("Stage 0 declined invalid extern replay fixture: {reason}")
        }
        topaz_kernel::KernelOutcome::ResourceLimit(limit) => {
            panic!("Stage 0 exhausted invalid extern replay fixture resource: {limit:?}")
        }
        topaz_kernel::KernelOutcome::CompilerFault { message } => {
            panic!("Stage 0 invalid extern replay compiler fault: {message}")
        }
    };
    assert!(
        self_preview.resolved.diagnostics.is_empty(),
        "self-host resolver rejected the declared extern module: {:?}",
        self_preview.resolved.diagnostics,
    );
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_checked = stage0_unit
        .checked
        .as_ref()
        .expect("Stage 0 checked invalid extern replay fixture");
    let stage0_diagnostics = stage0_checked
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        stage0_diagnostics,
        vec![(
            topaz_diag::extern_codes::REPLAY,
            "extern module `host.math` has an invalid deterministic replay binding: fixture row has an invalid result shape",
        )],
    );
}

#[test]
fn resolver_reserved_topaz_diagnostic_matches_stage0() {
    let request = resolver_request();
    let self_preview = preview_resolved(&ReservedTopazResolverHost, request.clone())
        .expect("self-host reserved topaz resolution");
    let stage0 = topaz_kernel::drive_checked(&ReservedTopazResolverHost, request);
    let stage0_unit = match stage0.outcome {
        topaz_kernel::KernelOutcome::Rejected(unit) => unit,
        topaz_kernel::KernelOutcome::Completed(_) => {
            panic!("Stage 0 admitted the reserved topaz root")
        }
        topaz_kernel::KernelOutcome::NeedHostFacts(queries) => {
            panic!("Stage 0 left reserved topaz facts pending: {queries:?}")
        }
        topaz_kernel::KernelOutcome::Declined { reason } => {
            panic!("Stage 0 declined reserved topaz fixture: {reason}")
        }
        topaz_kernel::KernelOutcome::ResourceLimit(limit) => {
            panic!("Stage 0 exhausted reserved topaz fixture resource: {limit:?}")
        }
        topaz_kernel::KernelOutcome::CompilerFault { message } => {
            panic!("Stage 0 reserved topaz compiler fault: {message}")
        }
    };
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0_unit
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        stage0_diagnostics,
        vec![(
            "TPZ3016",
            "the module path root `topaz` is reserved; user modules cannot live under it",
        )],
    );
}

#[test]
fn resolver_overlapping_cycles_emit_one_canonical_scc_diagnostic_like_stage0() {
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&OverlappingCycleResolverHost, request.clone())
        .expect("self-host overlapping cycle resolution");
    let stage0 = topaz_kernel::drive_checked(&OverlappingCycleResolverHost, request);
    let stage0_unit = match stage0.outcome {
        topaz_kernel::KernelOutcome::Rejected(unit) => unit,
        topaz_kernel::KernelOutcome::Completed(_) => {
            panic!("Stage 0 admitted an overlapping import cycle")
        }
        topaz_kernel::KernelOutcome::NeedHostFacts(queries) => {
            panic!("Stage 0 left overlapping cycle facts pending: {queries:?}")
        }
        topaz_kernel::KernelOutcome::Declined { reason } => {
            panic!("Stage 0 declined overlapping cycle fixture: {reason}")
        }
        topaz_kernel::KernelOutcome::ResourceLimit(limit) => {
            panic!("Stage 0 exhausted overlapping cycle fixture resource: {limit:?}")
        }
        topaz_kernel::KernelOutcome::CompilerFault { message } => {
            panic!("Stage 0 overlapping cycle compiler fault: {message}")
        }
    };
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0_unit
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        stage0_diagnostics,
        vec![("TPZ3006", "import cycle: a -> b -> a")],
    );
}

#[test]
fn resolver_entry_identity_strips_exactly_one_tpz_suffix() {
    let host = ResolverSourcesHost {
        a_source: "let value = 1\n",
        b_source: None,
        c_source: None,
        alias_class: "repeated-entry-suffix",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host repeated-suffix entry resolution");
    let stage0 = stage0_resolved_unit(&host, request);
    let self_entry = self_preview
        .modules
        .iter()
        .find(|module| module.entry)
        .expect("self-host entry module");
    let stage0_entry = stage0
        .resolved
        .modules
        .iter()
        .find(|module| module.is_entry)
        .expect("Stage 0 entry module");
    assert_eq!(self_entry.identity, "a.tpz");
    assert_eq!(stage0_entry.identity, "a.tpz");
}

#[test]
fn resolver_rejects_the_entry_file_as_its_own_source_root() {
    let host = ResolverSourcesHost {
        a_source: "let value = 1\n",
        b_source: None,
        c_source: None,
        alias_class: "entry-file-root",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root/a.tpz"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host entry-file source-root rejection");
    let stage0 = stage0_resolved_unit(&host, request);
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        stage0_diagnostics,
        vec![(
            "TPZ3002",
            "the source root `root/a.tpz` must be a directory containing the entry, not the entry file itself",
        )],
    );
}

#[test]
fn resolver_rejects_two_module_identities_for_one_physical_file() {
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&PhysicalAliasResolverHost, request.clone())
        .expect("self-host physical alias resolution");
    let stage0 = stage0_resolved_unit(&PhysicalAliasResolverHost, request);
    let expected = "the modules `b` and `c` resolve to the same physical file; one physical file cannot have two module identities";

    let self_collision = self_preview
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == "TPZ3004")
        .expect("self-host physical module collision");
    assert_eq!(self_collision.message, expected);
    let stage0_collision = stage0
        .resolved
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == "TPZ3004")
        .expect("Stage 0 physical module collision");
    assert_eq!(stage0_collision.message, expected);
}

#[test]
fn resolver_canonical_cycle_message_and_primary_span_match_stage0() {
    let host = ResolverSourcesHost {
        a_source: "import c\nimport b\n",
        b_source: Some("import a\n"),
        c_source: Some("import a\n"),
        alias_class: "canonical-cycle-span",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host canonical cycle span resolution");
    let stage0 = topaz_kernel::drive_checked(&host, request);
    let stage0_unit = match stage0.outcome {
        topaz_kernel::KernelOutcome::Rejected(unit) => unit,
        topaz_kernel::KernelOutcome::Completed(_) => {
            panic!("Stage 0 admitted the canonical cycle span fixture")
        }
        topaz_kernel::KernelOutcome::NeedHostFacts(queries) => {
            panic!("Stage 0 left canonical cycle span facts pending: {queries:?}")
        }
        topaz_kernel::KernelOutcome::Declined { reason } => {
            panic!("Stage 0 declined canonical cycle span fixture: {reason}")
        }
        topaz_kernel::KernelOutcome::ResourceLimit(limit) => {
            panic!("Stage 0 exhausted canonical cycle span fixture resource: {limit:?}")
        }
        topaz_kernel::KernelOutcome::CompilerFault { message } => {
            panic!("Stage 0 canonical cycle span compiler fault: {message}")
        }
    };
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str(),
                diagnostic.message.as_str(),
                diagnostic.lo,
                diagnostic.hi,
            )
        })
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0_unit
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str(),
                diagnostic.message.as_str(),
                diagnostic.primary.span.lo,
                diagnostic.primary.span.hi,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        stage0_diagnostics,
        vec![("TPZ3006", "import cycle: a -> b -> a", 16, 17)],
    );
}

#[test]
fn resolver_mixed_self_and_multi_module_cycle_matches_stage0_scc_policy() {
    let host = ResolverSourcesHost {
        a_source: "import a\nimport b\n",
        b_source: Some("import a\n"),
        c_source: None,
        alias_class: "mixed-cycle",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host mixed self and multi-module cycle resolution");
    let stage0 = topaz_kernel::drive_checked(&host, request);
    let stage0_unit = match stage0.outcome {
        topaz_kernel::KernelOutcome::Rejected(unit) => unit,
        topaz_kernel::KernelOutcome::Completed(_) => {
            panic!("Stage 0 admitted the mixed self and multi-module cycle fixture")
        }
        topaz_kernel::KernelOutcome::NeedHostFacts(queries) => {
            panic!("Stage 0 left mixed cycle facts pending: {queries:?}")
        }
        topaz_kernel::KernelOutcome::Declined { reason } => {
            panic!("Stage 0 declined the mixed cycle fixture: {reason}")
        }
        topaz_kernel::KernelOutcome::ResourceLimit(limit) => {
            panic!("Stage 0 exhausted the mixed cycle fixture resource: {limit:?}")
        }
        topaz_kernel::KernelOutcome::CompilerFault { message } => {
            panic!("Stage 0 mixed cycle compiler fault: {message}")
        }
    };
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str(),
                diagnostic.message.as_str(),
                diagnostic.lo,
                diagnostic.hi,
            )
        })
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0_unit
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str(),
                diagnostic.message.as_str(),
                diagnostic.primary.span.lo,
                diagnostic.primary.span.hi,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        stage0_diagnostics,
        vec![("TPZ3006", "import cycle: a -> b -> a", 16, 17)],
    );
}

#[test]
fn resolver_preserves_binding_collision_and_export_mut_diagnostics_like_stage0() {
    let host = ResolverSourcesHost {
        a_source: concat!("let [x, x] | [x, x] = [1, 2]\n", "export let mut x = 3\n",),
        b_source: None,
        c_source: None,
        alias_class: "repeated-or-binding",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host repeated or-pattern binding resolution");
    let stage0 = topaz_kernel::drive_checked(&host, request);
    let stage0_unit = match stage0.outcome {
        topaz_kernel::KernelOutcome::Rejected(unit) => unit,
        topaz_kernel::KernelOutcome::Completed(_) => {
            panic!("Stage 0 admitted repeated bindings inside or-pattern alternatives")
        }
        topaz_kernel::KernelOutcome::NeedHostFacts(queries) => {
            panic!("Stage 0 left repeated or-pattern binding facts pending: {queries:?}")
        }
        topaz_kernel::KernelOutcome::Declined { reason } => {
            panic!("Stage 0 declined repeated or-pattern binding fixture: {reason}")
        }
        topaz_kernel::KernelOutcome::ResourceLimit(limit) => {
            panic!("Stage 0 exhausted repeated or-pattern binding resource: {limit:?}")
        }
        topaz_kernel::KernelOutcome::CompilerFault { message } => {
            panic!("Stage 0 repeated or-pattern binding compiler fault: {message}")
        }
    };
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str(),
                diagnostic.message.as_str(),
                diagnostic.lo,
                diagnostic.hi,
            )
        })
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0_unit
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str(),
                diagnostic.message.as_str(),
                diagnostic.primary.span.lo,
                diagnostic.primary.span.hi,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        stage0_diagnostics,
        vec![
            (
                "TPZ3008",
                "`x` is already bound at this module's top level (first binding at byte 5)",
                8,
                9,
            ),
            (
                "TPZ3008",
                "`x` is already bound at this module's top level (first binding at byte 5)",
                17,
                18,
            ),
            (
                "TPZ3011",
                "`export let mut` is a static error: exported bindings are immutable views",
                44,
                45,
            ),
            (
                "TPZ3008",
                "`x` is already bound at this module's top level (first binding at byte 5)",
                44,
                45,
            ),
        ],
    );
}

#[test]
fn resolver_protocol_name_does_not_collide_with_value_binding_like_stage0() {
    let host = ResolverSourcesHost {
        a_source: concat!(
            "protocol Measure { function measure(value: Self) -> int }\n",
            "let Measure = 42\n",
        ),
        b_source: None,
        c_source: None,
        alias_class: "protocol-value-namespace",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host protocol and value namespace resolution");
    let stage0 = stage0_resolved_unit(&host, request);
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert!(stage0_diagnostics.is_empty());
}

#[test]
fn resolver_export_protocol_does_not_create_an_import_surface_like_stage0() {
    let host = ResolverSourcesHost {
        a_source: "import b { Measure }\n",
        b_source: Some("export protocol Measure { function measure(value: Self) -> int }\n"),
        c_source: None,
        alias_class: "export-protocol-surface",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host export protocol surface resolution");
    let stage0 = stage0_resolved_unit(&host, request);
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        stage0_diagnostics,
        vec![
            ("TPZ2001", "expected a statement separator"),
            (
                "TPZ3007",
                "imported module `b` contains a top-level free statement; free statements are entry-only (the same file is valid as an entry)",
            ),
        ],
    );
}

#[test]
fn resolver_selected_type_alias_and_local_reference_targets_match_stage0() {
    let host = ResolverSourcesHost {
        a_source: concat!(
            "import b as B\n",
            "import c { User as ImportedUser }\n",
            "record Config {\n",
            "  value: int = B.seed,\n",
            "}\n",
            "function localShadow() -> int {\n",
            "  let value = 1\n",
            "  let value = 2\n",
            "  value\n",
            "}\n",
            "function typeShadow(input: string) -> string {\n",
            "  type Local = int\n",
            "  type Local = string\n",
            "  let observed: Local = input\n",
            "  observed\n",
            "}\n",
            "let imported: ImportedUser = 0\n",
        ),
        b_source: Some(concat!(
            "let seed = 41\n",
            "export function visible() -> int { seed }\n",
        )),
        c_source: Some("export type User = int\n"),
        alias_class: "record-default-private-runtime-value",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host private runtime record-default resolution");
    let stage0 = stage0_resolved_unit(&host, request);
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert!(stage0_diagnostics.is_empty());
    let mut self_targets = self_preview
        .references
        .iter()
        .filter(|reference| {
            matches!(
                reference.name.as_str(),
                "B.seed" | "ImportedUser" | "Local" | "value"
            )
        })
        .map(|reference| {
            (
                reference.name.as_str(),
                reference.target_module.as_deref(),
                reference.target_name.as_deref(),
                reference.target_lo,
                reference.target_hi,
            )
        })
        .collect::<Vec<_>>();
    let mut stage0_targets = stage0
        .resolved
        .name_facts
        .references
        .iter()
        .filter(|reference| {
            matches!(
                reference.name.as_str(),
                "B.seed" | "ImportedUser" | "Local" | "value"
            )
        })
        .map(|reference| {
            let span = reference
                .target_span
                .expect("resolved reference target span");
            (
                reference.name.as_str(),
                reference.target_module.as_deref(),
                reference.target_name.as_deref(),
                span.lo,
                span.hi,
            )
        })
        .collect::<Vec<_>>();
    self_targets.sort();
    stage0_targets.sort();
    assert_eq!(self_targets, stage0_targets);
    assert_eq!(
        stage0_targets,
        [
            ("B.seed", Some("b"), Some("seed"), 4, 8),
            ("ImportedUser", Some("a"), Some("ImportedUser"), 33, 45),
            ("Local", Some("a"), Some("Local"), 236, 241),
            ("value", Some("a"), Some("value"), 143, 148),
        ],
    );
}

#[test]
fn resolver_private_namespace_value_exception_stays_record_default_immutable_only() {
    let host = ResolverSourcesHost {
        a_source: concat!(
            "import b as B\n",
            "let outside = B.seed\n",
            "record Config {\n",
            "  value: int = B.mutableSeed,\n",
            "}\n",
        ),
        b_source: Some(concat!(
            "let seed = 41\n",
            "let mut mutableSeed = 42\n",
            "export function visible() -> int { seed }\n",
        )),
        c_source: None,
        alias_class: "record-default-private-runtime-boundary",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host private runtime record-default boundary resolution");
    let stage0 = stage0_resolved_unit(&host, request);
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        stage0_diagnostics,
        vec![
            ("TPZ3009", "`seed` is not exported by `b`"),
            ("TPZ3009", "`mutableSeed` is not exported by `b`"),
        ],
    );
}

#[test]
fn resolver_preserves_namespace_member_targets_and_keyword_kind_after_collisions_like_stage0() {
    let host = ResolverSourcesHost {
        a_source: concat!(
            "import b as B\n",
            "import c as C\n",
            "let observed: C.Visible = 1\n",
            "let rejected: C.Amount = 1\n",
            "let keyword = C.if\n",
            "record Config {\n",
            "  callback: (int) -> int = (value: B.hidden) => value,\n",
            "}\n",
        ),
        b_source: Some("let hidden = 1\n"),
        c_source: Some(concat!(
            "let Visible = 1\n",
            "export type Visible = int\n",
            "export const Amount = 1\n",
        )),
        alias_class: "namespace-member-target-and-kind-after-binding-collision",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host export surface resolution after binding collision");
    let stage0 = stage0_resolved_unit(&host, request);
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let self_exports = self_preview
        .exports
        .iter()
        .map(|export| {
            (
                self_preview.modules[export.module_index].identity.as_str(),
                export.name.as_str(),
                export.namespace.as_str(),
                export.declaration_lo,
                export.declaration_hi,
            )
        })
        .collect::<Vec<_>>();
    let stage0_exports = stage0
        .resolved
        .name_facts
        .exports
        .iter()
        .map(|export| {
            let module = stage0
                .resolved
                .modules
                .iter()
                .find(|module| module.file == export.file)
                .expect("export source module");
            let namespace = match export.namespace {
                topaz_resolve::ResolvedNamespace::Value => "value",
                topaz_resolve::ResolvedNamespace::Type => "type",
                topaz_resolve::ResolvedNamespace::Module => "module",
            };
            (
                module.identity.as_str(),
                export.name.as_str(),
                namespace,
                export.declaration_span.lo,
                export.declaration_span.hi,
            )
        })
        .collect::<Vec<_>>();
    let self_references = self_preview
        .references
        .iter()
        .filter(|reference| matches!(reference.name.as_str(), "C.Amount" | "C.if" | "B.hidden"))
        .map(|reference| {
            (
                reference.name.as_str(),
                reference.namespace.as_str(),
                reference.lo,
                reference.hi,
                reference.target_module.as_deref(),
                reference.target_name.as_deref(),
                reference.target_namespace.as_deref(),
                reference.target_lo,
                reference.target_hi,
            )
        })
        .collect::<Vec<_>>();
    let stage0_references = stage0
        .resolved
        .name_facts
        .references
        .iter()
        .filter(|reference| matches!(reference.name.as_str(), "C.Amount" | "C.if" | "B.hidden"))
        .map(|reference| {
            let namespace = match reference.namespace {
                topaz_resolve::ResolvedNamespace::Value => "value",
                topaz_resolve::ResolvedNamespace::Type => "type",
                topaz_resolve::ResolvedNamespace::Module => "module",
            };
            let target_namespace = reference.target_namespace.map(|namespace| match namespace {
                topaz_resolve::ResolvedNamespace::Value => "value",
                topaz_resolve::ResolvedNamespace::Type => "type",
                topaz_resolve::ResolvedNamespace::Module => "module",
            });
            let (target_lo, target_hi) = reference
                .target_span
                .map_or((0, 0), |span| (span.lo, span.hi));
            (
                reference.name.as_str(),
                namespace,
                reference.span.lo,
                reference.span.hi,
                reference.target_module.as_deref(),
                reference.target_name.as_deref(),
                target_namespace,
                target_lo,
                target_hi,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        (self_diagnostics, self_exports, self_references),
        (
            stage0_diagnostics.clone(),
            stage0_exports.clone(),
            stage0_references.clone(),
        ),
    );
    assert_eq!(
        (stage0_diagnostics, stage0_exports, stage0_references),
        (
            vec![
                (
                    "TPZ3008",
                    "`Visible` is already bound at this module's top level (first binding at byte 4)",
                ),
                (
                    "TPZ3010",
                    "`b` exports nothing; v5.2 has no side-effect-only imports",
                ),
                (
                    "TPZ3013",
                    "`Amount` is exported by `c` but is not a type alias",
                ),
                (
                    "TPZ3013",
                    "namespace members are exported declarations and cannot be keyword-named (`C.if`)",
                ),
                ("TPZ3009", "`hidden` is not exported by `b`"),
            ],
            vec![
                ("c", "Amount", "value", 55, 61),
                ("c", "Visible", "type", 4, 11),
            ],
            vec![
                (
                    "C.Amount",
                    "type",
                    72,
                    78,
                    Some("c"),
                    Some("Amount"),
                    Some("type"),
                    55,
                    61,
                ),
                ("C.if", "value", 99, 101, None, None, None, 0, 0,),
                (
                    "B.hidden",
                    "type",
                    155,
                    161,
                    Some("b"),
                    Some("hidden"),
                    Some("type"),
                    4,
                    10,
                ),
            ],
        ),
    );
}

#[test]
fn resolver_import_surface_suggestions_match_stage0() {
    let host = ResolverSourcesHost {
        a_source: concat!(
            "import b { mesure }\n",
            "import c as C\n",
            "let observed = C.mesure(1)\n",
            "let typed: C.Distnace = 1\n",
        ),
        b_source: Some(concat!(
            "export function measure(value: int) -> int { value }\n",
            "export type Distance = int\n",
        )),
        c_source: Some(concat!(
            "export function measure(value: int) -> int { value }\n",
            "export type Distance = int\n",
        )),
        alias_class: "import-surface-suggestions",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host import-surface suggestion resolution");
    let stage0 = stage0_resolved_unit(&host, request);
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        stage0_diagnostics,
        vec![
            (
                "TPZ3009",
                "`mesure` is not exported by `b`; did you mean `measure`?",
            ),
            (
                "TPZ3009",
                "`mesure` is not exported by `c`; did you mean `measure`?",
            ),
            (
                "TPZ3009",
                "`Distnace` is not exported by `c`; did you mean `Distance`?",
            ),
        ],
    );
}

#[test]
fn resolver_private_type_in_exported_surface_matches_stage0() {
    let host = ResolverSourcesHost {
        a_source: "import b\n",
        b_source: Some(concat!(
            "type Internal = { id: int }\n",
            "record Hidden { id: int }\n",
            "export function load(value: Internal) -> Hidden { { id: value.id } }\n",
            "export type Mapper = (Internal) -> Hidden\n",
            "export enum Wrapped { Pair(Internal, Hidden) }\n",
            "export record Surface { first: Internal, second: Hidden }\n",
            "export newtype Identifier = Internal\n",
            "export let exposed: Hidden = { id: 1 }\n",
        )),
        c_source: None,
        alias_class: "private-type-in-exported-surface",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host private type in exported surface resolution");
    let stage0 = stage0_resolved_unit(&host, request);
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
            stage0_diagnostics,
            [
                "Internal", "Hidden", "Internal", "Hidden", "Internal", "Hidden", "Internal",
                "Hidden", "Internal",
            ]
            .map(|name| {
                (
                    "TPZ3014",
                    if name == "Internal" {
                        "`Internal` is a module-private type and may not appear in an exported public surface; export the type or use an inline structural type"
                    } else {
                        "`Hidden` is a module-private type and may not appear in an exported public surface; export the type or use an inline structural type"
                    },
                )
            })
            .to_vec(),
        );
}

#[test]
fn resolver_exported_type_in_public_surface_remains_resolvable() {
    let host = ResolverSourcesHost {
        a_source: "import b\n",
        b_source: Some(concat!(
            "export type Public = { id: int }\n",
            "export function load(value: Public) -> Public { value }\n",
            "export type Mapper = (Public) -> Public\n",
            "export enum Wrapped { Pair(Public, Public) }\n",
            "export record Surface { first: Public, second: Public }\n",
            "export newtype Identifier = Public\n",
            "export let exposed: Public = { id: 1 }\n",
        )),
        c_source: None,
        alias_class: "exported-type-in-public-surface",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host exported type in public surface resolution");
    let stage0 = stage0_resolved_unit(&host, request);
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert!(stage0_diagnostics.is_empty());
}

#[test]
fn resolver_imported_initializer_forward_reference_matches_stage0() {
    let host = ResolverSourcesHost {
        a_source: "import b\n",
        b_source: Some(concat!(
            "let first = later\n",
            "let tagged = sql\"{later}\"\n",
            "let later = 5\n",
            "let selfRead = selfRead\n",
            "let directCall = after()\n",
            "let valueContainer = {\n",
            "  using nested = afterUsingValue { nested }\n",
            "  0\n",
            "}\n",
            "let bodyContainer = {\n",
            "  using nested = 0 { afterUsingBody }\n",
            "  0\n",
            "}\n",
            "let shadowedUsing = {\n",
            "  using afterUsingBody = 0 { afterUsingBody }\n",
            "  0\n",
            "}\n",
            "function after() -> int { later }\n",
            "let afterUsingValue = 6\n",
            "let afterUsingBody = 7\n",
            "export function visible() -> int { first }\n",
        )),
        c_source: None,
        alias_class: "imported-initializer-forward-reference",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let self_preview = preview_resolved(&host, request.clone())
        .expect("self-host imported initializer forward-reference resolution");
    let stage0 = stage0_resolved_unit(&host, request);
    let self_diagnostics = self_preview
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str(),
                diagnostic.message.as_str(),
                diagnostic.lo,
                diagnostic.hi,
            )
        })
        .collect::<Vec<_>>();
    let stage0_diagnostics = stage0
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str(),
                diagnostic.message.as_str(),
                diagnostic.primary.span.lo,
                diagnostic.primary.span.hi,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    let stage0_messages = stage0_diagnostics
        .iter()
        .map(|(code, message, _, _)| (*code, *message))
        .collect::<Vec<_>>();
    assert_eq!(
        stage0_messages,
        vec![
            (
                "TPZ3018",
                "the initializer of `first` reaches `later`, whose initializer has not completed (v5.2 defines no partially initialized module binding)",
            ),
            (
                "TPZ3018",
                "the initializer of `tagged` reaches `later`, whose initializer has not completed (v5.2 defines no partially initialized module binding)",
            ),
            (
                "TPZ3018",
                "the initializer of `selfRead` reaches `selfRead`, whose initializer has not completed (v5.2 defines no partially initialized module binding)",
            ),
            (
                "TPZ3018",
                "the initializer of `directCall` reaches `after`, whose initializer has not completed (v5.2 defines no partially initialized module binding)",
            ),
            (
                "TPZ3018",
                "the initializer of `valueContainer` reaches `afterUsingValue`, whose initializer has not completed (v5.2 defines no partially initialized module binding)",
            ),
            (
                "TPZ3018",
                "the initializer of `bodyContainer` reaches `afterUsingBody`, whose initializer has not completed (v5.2 defines no partially initialized module binding)",
            ),
        ],
    );
}

#[test]
fn resolver_nominal_pattern_reports_its_initialized_bindings() {
    let host = ResolverSourcesHost {
        a_source: "import b\n",
        b_source: Some(concat!(
            "record Pair { left: int, right: int }\n",
            "let Pair { left, right } = Pair { left: later, right: 0 }\n",
            "let later = 1\n",
            "export function visible() -> int { left + right }\n",
        )),
        c_source: None,
        alias_class: "nominal-pattern-initializer-owner",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let preview =
        preview_resolved(&host, request).expect("self-host nominal-pattern initializer resolution");
    let diagnostics = preview
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        diagnostics,
        vec![(
            "TPZ3018",
            "the initializer of `left, right` reaches `later`, whose initializer has not completed (v5.2 defines no partially initialized module binding)",
        )],
    );
}

#[test]
fn resolver_initializer_delayed_const_and_entry_boundaries_match_stage0() {
    for (alias_class, a_source, b_source) in [
        (
            "imported-initializer-delayed-and-const-boundaries",
            "import b\n",
            concat!(
                "function use(value: int) -> int { value }\n",
                "function defaulted(value: int = later) -> int { value }\n",
                "let earlier = 1\n",
                "let fromEarlier = earlier\n",
                "let fromConst = eventual\n",
                "let lambda = () => later\n",
                "let calledLambda = (() => later)()\n",
                "let branch = if true { later } else { 0 }\n",
                "let arm = match 0 { case _ => later }\n",
                "let loop = for value in [1] { later }\n",
                "let comprehension = [for value in [1] => later]\n",
                "let record = { read: () => later }\n",
                "let shadowed = { let later = 1\n later }\n",
                "let deferred = { defer { later }\n 0 }\n",
                "let shortAnd = false && laterBool\n",
                "let shortOr = true || laterBool\n",
                "let present: Option<int> = Some(1)\n",
                "let coalesced: int = present ?? later\n",
                "let receiver: Option<string> = None\n",
                "let optionalCall: string = receiver?.replace(laterText, \"y\") ?? \"skipped\"\n",
                "let later = 2\n",
                "let laterBool = true\n",
                "let laterText = \"x\"\n",
                "const eventual = 3\n",
                "export function visible() -> int { use(fromEarlier + fromConst) }\n",
            ),
        ),
        (
            "entry-initializer-role-boundary",
            concat!("let first = later\n", "let later = 5\n",),
            "export let value = 1\n",
        ),
    ] {
        let host = ResolverSourcesHost {
            a_source,
            b_source: Some(b_source),
            c_source: None,
            alias_class,
        };
        let request = topaz_kernel::KernelRequest::checked(
            "root/a.tpz",
            Some("root"),
            LangVersion::CURRENT,
            topaz_kernel::PackageFacts::standalone(),
        )
        .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
        let self_preview = preview_resolved(&host, request.clone())
            .expect("self-host initializer boundary resolution");
        let stage0 = stage0_resolved_unit(&host, request);
        let self_diagnostics = self_preview
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code.as_str(),
                    diagnostic.message.as_str(),
                    diagnostic.lo,
                    diagnostic.hi,
                )
            })
            .collect::<Vec<_>>();
        let stage0_diagnostics = stage0
            .resolved
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code.as_str(),
                    diagnostic.message.as_str(),
                    diagnostic.primary.span.lo,
                    diagnostic.primary.span.hi,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(self_diagnostics, stage0_diagnostics, "{alias_class}");
        assert!(stage0_diagnostics.is_empty(), "{alias_class}");
    }
}

#[test]
fn resolver_discovery_uses_the_lexer_unicode_identifier_authority() {
    let preview = preview_resolved(&UnicodeResolverFixtureHost, resolver_request())
        .expect("Unicode resolver preview");
    assert!(preview.diagnostics.is_empty(), "{:?}", preview.diagnostics);
    let identities = preview
        .modules
        .iter()
        .map(|module| module.identity.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        identities,
        std::collections::BTreeSet::from(["main", "модуль", "한글.모듈", "🚀"])
    );
}
