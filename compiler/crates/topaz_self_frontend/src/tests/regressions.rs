use super::*;

#[test]
fn resolver_unicode_collision_keys_and_details_match_stage0() {
    for (kind, entry_source, exact_path, colliding_path) in [
        (
            UnicodeCollisionKind::CaseFold,
            "import straße { value }\nlet answer = value\n",
            "root/straße.tpz",
            "root/strasse.tpz",
        ),
        (
            UnicodeCollisionKind::Canonical,
            "import café { value }\nlet answer = value\n",
            "root/café.tpz",
            "root/cafe\u{301}.tpz",
        ),
    ] {
        let self_preview =
            preview_resolved(&UnicodeCollisionResolverHost(kind), resolver_request())
                .expect("self-host Unicode collision resolution");

        let mut provider = topaz_resolve::InMemoryProvider::new();
        provider.add_file("root/main.tpz", entry_source);
        provider.add_file(exact_path, "export const value = 42\n");
        provider.add_file(colliding_path, "export const value = 41\n");
        let stage0 = topaz_resolve::resolve_with_version(
            &provider,
            "root/main.tpz",
            Some("root"),
            LangVersion::CURRENT,
        );
        let self_diagnostics = self_preview
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
            .collect::<Vec<_>>();
        let stage0_diagnostics = stage0
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(self_diagnostics, stage0_diagnostics);
        assert_eq!(
            stage0_diagnostics
                .iter()
                .map(|(code, _)| *code)
                .collect::<Vec<_>>(),
            vec!["TPZ3004"],
        );
    }
}

#[test]
fn resolver_parent_path_rejection_uses_exact_segments_like_stage0() {
    let source = "let answer = 42\n";
    for (entry, root) in [
        ("root/main..tpz", "root"),
        ("root..dir/main.tpz", "root..dir"),
    ] {
        let request = topaz_kernel::KernelRequest::checked(
            entry,
            Some(root),
            LangVersion::CURRENT,
            topaz_kernel::PackageFacts::standalone(),
        )
        .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
        let self_preview = preview_resolved(&DottedEntryResolverHost, request)
            .expect("self-host dotted path resolution");

        let mut provider = topaz_resolve::InMemoryProvider::new();
        provider.add_file(entry, source);
        let stage0 =
            topaz_resolve::resolve_with_version(&provider, entry, Some(root), LangVersion::CURRENT);

        assert!(stage0.diagnostics.is_empty(), "{:?}", stage0.diagnostics);
        assert!(
            self_preview.diagnostics.is_empty(),
            "{:?}",
            self_preview.diagnostics
        );
        assert_eq!(
            self_preview
                .modules
                .iter()
                .map(|module| module.identity.as_str())
                .collect::<Vec<_>>(),
            stage0
                .modules
                .iter()
                .map(|module| module.identity.as_str())
                .collect::<Vec<_>>(),
        );
    }

    let invalid_request = topaz_kernel::KernelRequest::checked(
        "root/../main.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Resolved);
    let invalid_self =
        preview_resolved(&NoFactHost, invalid_request).expect("self-host parent entry rejection");
    let provider = topaz_resolve::InMemoryProvider::new();
    let invalid_stage0 = topaz_resolve::resolve_with_version(
        &provider,
        "root/../main.tpz",
        Some("root"),
        LangVersion::CURRENT,
    );
    assert!(
        provider.reads().is_empty(),
        "Stage 0 parent-segment rejection must precede source reads"
    );
    let stage0_diagnostics = invalid_stage0
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    let self_diagnostics = invalid_self
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(self_diagnostics, stage0_diagnostics);
    assert_eq!(
        self_diagnostics,
        vec![(
            "TPZ3002",
            "entry and root paths must not contain parent (`..`) segments",
        )],
    );
}

#[test]
fn resolver_records_one_declaration_for_a_nested_protocol() {
    let preview = typed_source(concat!(
        "function f() -> int {\n",
        "  protocol P { function value(item: Self) -> int }\n",
        "  0\n",
        "}\n",
        "f()\n",
    ));
    let declarations = preview
        .resolved
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.name == "P"
                && declaration.namespace == "type"
                && declaration.declaration_kind == "protocol"
        })
        .collect::<Vec<_>>();
    assert_eq!(declarations.len(), 1, "{declarations:#?}");
    assert!(declarations[0].scope_ordinal > 0);
}

#[test]
fn resolver_preview_enforces_the_source_fact_limit() {
    let mut request = resolver_request();
    request.budgets_mut().max_source_facts = 1;
    let error = match preview_resolved(&ResolverFixtureHost::new(), request) {
        Ok(_) => panic!("second source fact must exceed the fixed budget"),
        Err(error) => error,
    };
    assert!(
        error
            .contains("front-end resolver preview source-fact resource limit: observed 2, limit 1"),
        "{error}"
    );
}

#[test]
fn checker_preview_types_a_resolved_two_module_unit_without_fallback() {
    let request = resolver_request().with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    let preview =
        preview_typed(&ResolverFixtureHost::new(), request).expect("typed preview result");
    assert_eq!(
        preview.resolved.request.terminal_phase(),
        topaz_kernel::TerminalPhase::Typed
    );
    assert!(!preview.nodes.is_empty());
    assert!(
        preview
            .nodes
            .iter()
            .all(|node| node.ambient || !node.ty.has_hole())
    );
    assert!(preview.calls.is_empty());
    assert!(preview.captures.is_empty());
}

#[test]
fn checker_local_generic_bounds_accept_imported_derived_nominals() {
    let host = ResolverSourcesHost {
        a_source: concat!(
            "import b as Other\n",
            "import c { User as ImportedUser, make }\n",
            "function label<T: Show>(value: T) -> string { Show.show(value) }\n",
            "let user: ImportedUser = make()\n",
            "let text: string = label(user)\n",
        ),
        b_source: Some("export record User derives Eq { id: int }\n"),
        c_source: Some(concat!(
            "export record User derives Show { name: string }\n",
            "export function make() -> User { User { name: \"Ada\" } }\n",
        )),
        alias_class: "imported-bound",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    let stage0 = topaz_kernel::drive_checked(&host, request.clone());
    let stage0_diagnostics = match &stage0.outcome {
        topaz_kernel::KernelOutcome::Rejected(unit) => unit
            .checked
            .as_ref()
            .map(|checked| checked.diagnostics.as_slice()),
        _ => None,
    };
    assert!(
        matches!(stage0.outcome, topaz_kernel::KernelOutcome::Completed(_)),
        "Stage 0 must accept the imported derived conformance: {stage0_diagnostics:?}"
    );

    let self_hosted = preview_typed(&host, request).expect("self-hosted imported bound preview");
    assert!(
        self_hosted.diagnostics.is_empty(),
        "self-host must accept the imported derived conformance: {:?}",
        self_hosted.diagnostics
    );
}

#[test]
fn checker_nominal_record_patterns_follow_imported_alias_identity() {
    let host = ResolverSourcesHost {
        a_source: concat!(
            "import b as Other\n",
            "import c { User as ImportedUser, make }\n",
            "function userName(value: ImportedUser) -> string {\n",
            "  match value {\n",
            "    case ImportedUser { name } => name\n",
            "  }\n",
            "}\n",
            "let text: string = userName(make())\n",
        ),
        b_source: Some("export record User { id: int }\n"),
        c_source: Some(concat!(
            "export record User { name: string }\n",
            "export function make() -> User { User { name: \"Ada\" } }\n",
        )),
        alias_class: "imported-record-pattern",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    let stage0 = topaz_kernel::drive_checked(&host, request.clone());
    let stage0_diagnostics = match &stage0.outcome {
        topaz_kernel::KernelOutcome::Rejected(unit) => unit
            .checked
            .as_ref()
            .map(|checked| checked.diagnostics.as_slice()),
        _ => None,
    };
    assert!(
        matches!(stage0.outcome, topaz_kernel::KernelOutcome::Completed(_)),
        "Stage 0 must accept the imported nominal pattern: {stage0_diagnostics:?}"
    );

    let self_hosted = preview_typed(&host, request).expect("self-hosted nominal pattern preview");
    assert!(
        self_hosted.diagnostics.is_empty(),
        "self-host must accept the imported nominal pattern: {:?}",
        self_hosted.diagnostics
    );

    let incomplete_host = ResolverSourcesHost {
        a_source: concat!(
            "import b as Other\n",
            "import c { User as ImportedUser }\n",
            "function userName(value: ImportedUser | Other.User) -> string {\n",
            "  match value {\n",
            "    case ImportedUser { name } => name\n",
            "  }\n",
            "}\n",
        ),
        b_source: Some("export record User { id: int }\n"),
        c_source: Some("export record User { name: string }\n"),
        alias_class: "same-spelled-record-coverage",
    };
    let incomplete_request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    let stage0 = topaz_kernel::drive_checked(&incomplete_host, incomplete_request.clone());
    let stage0_codes = match stage0.outcome {
        topaz_kernel::KernelOutcome::Rejected(unit) => unit
            .checked
            .expect("Stage 0 checked incomplete nominal match")
            .diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.code.to_string())
            .collect::<Vec<_>>(),
        _ => panic!("Stage 0 must reject the incomplete nominal match"),
    };
    let self_codes = preview_typed(&incomplete_host, incomplete_request)
        .expect("self-hosted incomplete nominal pattern preview")
        .diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();
    assert_eq!(stage0_codes, vec!["TPZ5021"]);
    assert_eq!(self_codes, stage0_codes);
}

#[test]
fn checker_newtype_patterns_follow_imported_alias_identity() {
    let host = ResolverSourcesHost {
        a_source: concat!(
            "import b as Other\n",
            "import c { UserId as Uid, make }\n",
            "function unwrap(value: Uid) -> int {\n",
            "  match value {\n",
            "    case Uid(inner) => inner\n",
            "  }\n",
            "}\n",
            "let number: int = unwrap(make())\n",
        ),
        b_source: Some("export newtype UserId = string\n"),
        c_source: Some(concat!(
            "export newtype UserId = int\n",
            "export function make() -> UserId { UserId(7) }\n",
        )),
        alias_class: "imported-newtype-pattern",
    };
    let request = topaz_kernel::KernelRequest::checked(
        "root/a.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    let stage0 = topaz_kernel::drive_checked(&host, request.clone());
    let stage0_diagnostics = match &stage0.outcome {
        topaz_kernel::KernelOutcome::Rejected(unit) => unit
            .checked
            .as_ref()
            .map(|checked| checked.diagnostics.as_slice()),
        _ => None,
    };
    assert!(
        matches!(stage0.outcome, topaz_kernel::KernelOutcome::Completed(_)),
        "Stage 0 must accept the imported newtype pattern: {stage0_diagnostics:?}"
    );

    let self_hosted = preview_typed(&host, request).expect("self-hosted newtype pattern preview");
    assert!(
        self_hosted.diagnostics.is_empty(),
        "self-host must accept the imported newtype pattern: {:?}",
        self_hosted.diagnostics
    );
}

#[test]
fn checker_generic_nominal_type_facts_keep_declaration_origins() {
    let preview = typed_source(
        "record Box<T> { value: T }\n\
             enum Maybe<T> { Present(T) }\n\
             newtype Id<T> = T\n\
             let boxed: Box<int> = Box { value: 7 }\n\
             let maybe: Maybe<int> = Maybe.Present(7)\n\
             let id: Id<int> = Id(7)\n",
    );
    let mut origins = preview
        .nodes
        .iter()
        .filter(|node| node.kind == topaz_hir::TypedNodeKind::Type)
        .filter_map(|node| match &node.ty {
            topaz_hir::SemanticType::Rigid { origin, .. } => Some(origin.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    origins.sort();
    origins.dedup();
    assert_eq!(origins.len(), 3, "{:#?}", preview.nodes);
}

#[test]
fn checker_generic_type_identity_and_receiver_type_arguments_are_lexical() {
    let local_annotation = concat!(
        "function outer<T: JSON>(outerValue: T) -> T {\n",
        "  let selected: T = outerValue\n",
        "  selected\n",
        "}\n",
    );
    assert!(stage0_checker_diagnostics(local_annotation).is_empty());
    assert!(self_checker_diagnostics(local_annotation).is_empty());

    let receiver = concat!(
        "let values: Array<int> = [1, 2]\n",
        "let mapped: Array<string> = values.map<string>((value: int) => \"{value}\")\n",
        "let sorted: Array<int> = values.sortedBy<int>((value: int) => value)\n",
        "let piped: Array<string> = ((value: int) => \"{value}\") |> values.map<string>()\n",
        "let optional: Option<int> = Some(1)\n",
        "let result: Result<int, string> = optional.okOr<string>(\"missing\")\n",
    );
    let receiver_stage0 = stage0_checker_diagnostics(receiver);
    let receiver_self = self_checker_diagnostics(receiver);
    assert!(receiver_stage0.is_empty(), "{receiver_stage0:?}");
    assert!(receiver_self.is_empty(), "{receiver_self:?}");

    let constructors = concat!(
        "let present = Some<int>(1)\n",
        "let success = Ok<int, string>(1)\n",
        "let failure = Err<int, string>(\"stop\")\n",
        "enum LiteralBox<T> { Empty, One(T) }\n",
        "function emptyLiteral<T>() -> LiteralBox<T> { LiteralBox.Empty }\n",
        "let contextualLiteral: LiteralBox<\"open\"> = emptyLiteral()\n",
        "let explicitLiteral: LiteralBox<\"open\"> = emptyLiteral<\"open\">()\n",
    );
    assert!(stage0_checker_diagnostics(constructors).is_empty());
    assert!(self_checker_diagnostics(constructors).is_empty());

    let monomorphic = concat!(
        "let values: Array<int> = [1, 2]\n",
        "let filtered = values.filter<int>((value: int) => true)\n",
    );
    let monomorphic_expected = vec![(
        "TPZ5512".to_string(),
        "this call is not generic, but 1 type argument was supplied".to_string(),
        47,
        87,
    )];
    assert_eq!(
        stage0_checker_diagnostics(monomorphic),
        monomorphic_expected,
    );
    assert_eq!(self_checker_diagnostics(monomorphic), monomorphic_expected);

    let conflicts = concat!(
        "let values: Array<int> = [1, 2]\n",
        "let mapped = values.map<int>((value: int) => \"{value}\")\n",
        "let success = Ok<string, int>(1)\n",
    );
    let expected = vec![
        (
            "TPZ5001".to_string(),
            "expected `(int) -> int`, found `(int) -> string`".to_string(),
            61,
            86,
        ),
        (
            "TPZ5001".to_string(),
            "expected `string`, found `int`".to_string(),
            118,
            119,
        ),
    ];
    assert_eq!(stage0_checker_diagnostics(conflicts), expected);
    assert_eq!(self_checker_diagnostics(conflicts), expected);

    let wrong_arity = concat!(
        "function choose<T, U>(left: T, right: U) -> T { left }\n",
        "let observed: string = choose<int>(\"left\", 1)\n",
    );
    let wrong_arity_expected = vec![(
        "TPZ5510".to_string(),
        "this call expects 2 type arguments, but 1 was supplied".to_string(),
        78,
        100,
    )];
    assert_eq!(
        stage0_checker_diagnostics(wrong_arity),
        wrong_arity_expected,
    );
    assert_eq!(self_checker_diagnostics(wrong_arity), wrong_arity_expected);

    let json_exact = "let observed: Result<string, string> = JSON.parseAs<string>(\"1\")\n";
    assert!(stage0_checker_diagnostics(json_exact).is_empty());
    assert!(self_checker_diagnostics(json_exact).is_empty());

    let json_wrong_arity =
        "let observed: Result<string, string> = JSON.parseAs<int, string>(\"1\")\n";
    let json_wrong_arity_expected = vec![(
        "TPZ5510".to_string(),
        "this call expects 1 type argument, but 2 were supplied".to_string(),
        39,
        69,
    )];
    assert_eq!(
        stage0_checker_diagnostics(json_wrong_arity),
        json_wrong_arity_expected,
    );
    assert_eq!(
        self_checker_diagnostics(json_wrong_arity),
        json_wrong_arity_expected,
    );

    let parenthesized_callee = concat!(
        "function identity<T>(value: T) -> T { value }\n",
        "let observed: int = (identity)<int>(1)\n",
    );
    let parenthesized_callee_expected = vec![(
        "TPZ5002".to_string(),
        "`int` is not bound".to_string(),
        77,
        80,
    )];
    let self_preview = typed_source(parenthesized_callee);
    let self_diagnostics = self_preview
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.clone(),
                diagnostic.message.clone(),
                diagnostic.lo,
                diagnostic.hi,
            )
        })
        .chain(self_preview.diagnostics.iter().map(|diagnostic| {
            (
                diagnostic.code.clone(),
                diagnostic.message.clone(),
                diagnostic.lo,
                diagnostic.hi,
            )
        }))
        .collect::<Vec<_>>();
    let request = topaz_kernel::KernelRequest::checked(
        "main.tpz",
        Some(""),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    let stage0 = stage0_resolved_unit(&SourceFixtureHost(parenthesized_callee), request);
    let stage0_diagnostics = stage0
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str().to_string(),
                diagnostic.message.clone(),
                diagnostic.primary.span.lo,
                diagnostic.primary.span.hi,
            )
        })
        .chain(stage0.checked.iter().flat_map(|checked| {
            checked.diagnostics.iter().map(|diagnostic| {
                (
                    diagnostic.code.as_str().to_string(),
                    diagnostic.message.clone(),
                    diagnostic.primary.span.lo,
                    diagnostic.primary.span.hi,
                )
            })
        }))
        .collect::<Vec<_>>();
    assert_eq!(stage0_diagnostics, parenthesized_callee_expected);
    assert_eq!(self_diagnostics, parenthesized_callee_expected);

    let comparison = concat!(
        "let f = 1\n",
        "let x = 2\n",
        "let y = 3\n",
        "let observed = f<x+y>()\n",
    );
    let comparison_expected = vec![(
        "TPZ5007".to_string(),
        "`bool` and `()` are not ordered comparable".to_string(),
        45,
        53,
    )];
    assert_eq!(stage0_checker_diagnostics(comparison), comparison_expected,);
    assert_eq!(self_checker_diagnostics(comparison), comparison_expected);
}
