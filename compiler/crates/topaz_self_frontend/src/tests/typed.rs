use super::*;

#[test]
fn checker_preview_types_the_locked_bootstrap_workload() {
    let request = topaz_kernel::KernelRequest::checked(
        "src/main.tpz",
        Some(""),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    let preview =
        preview_typed(&BootstrapFixtureHost::new(), request).expect("bootstrap typed preview");
    assert_eq!(preview.resolved.modules.len(), 5);
    assert!(!preview.nodes.is_empty());
    assert!(!preview.calls.is_empty());
    let hole_nodes = preview
        .nodes
        .iter()
        .filter(|node| node.ambient || node.ty.has_hole())
        .collect::<Vec<_>>();
    assert!(hole_nodes.is_empty(), "{hole_nodes:#?}");
    let hole_calls = preview
        .calls
        .iter()
        .filter(|call| call.ambient || call.callee_type.has_hole() || call.result_type.has_hole())
        .collect::<Vec<_>>();
    assert!(hole_calls.is_empty(), "{hole_calls:#?}");
    let bundle = topaz_kernel::build_typed_preview_observation(preview.observation_input())
        .expect("typed preview observation");
    bundle.validate().expect("valid typed preview observation");
}

#[test]
fn checker_preview_rejects_binding_mismatch_with_canonical_diagnostic() {
    let request = topaz_kernel::KernelRequest::checked(
        "main.tpz",
        Some(""),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    let preview = preview_typed(&TypeMismatchFixtureHost, request).expect("rejected typed preview");
    assert_eq!(
        preview.diagnostics.len(),
        1,
        "unexpected typed rows: nodes={:#?}, calls={:#?}, captures={:#?}",
        preview.nodes,
        preview.calls,
        preview.captures
    );
    assert!(preview.nodes.is_empty());
    assert!(preview.calls.is_empty());
    assert!(preview.captures.is_empty());
    let diagnostic = &preview.diagnostics[0];
    assert_eq!(diagnostic.code, "TPZ5001");
    assert_eq!(diagnostic.message, "expected `int`, found `string`");
    assert_eq!((diagnostic.lo, diagnostic.hi), (18, 22));
    let bundle = topaz_kernel::build_typed_preview_observation(preview.observation_input())
        .expect("rejected typed preview observation");
    bundle.validate().expect("valid rejected typed observation");
    let diagnostics = bundle
        .files
        .iter()
        .find(|file| file.path == "diagnostics.jsonl")
        .expect("diagnostics projection");
    let text = std::str::from_utf8(&diagnostics.bytes).expect("diagnostics UTF-8");
    assert_eq!(
        text,
        concat!(
            "{\"code\":\"TPZ5001\",\"message\":\"expected `int`, found `string`\",",
            "\"notes\":[],\"ordinal\":0,\"primary\":{\"message\":\"\",\"span\":",
            "{\"hi\":22,\"lo\":18,\"sourceId\":",
            "\"s:4409e0809545cd18e4bb46d60d85fc5fe1a9bfd8424282743862135da1eec0e1\"}},",
            "\"producerPhase\":\"front-end\",\"profileRule\":null,",
            "\"schema\":\"topaz.compiler.diagnostics/v1\",\"secondary\":[],",
            "\"severity\":\"error\"}\n"
        )
    );
    let response = bundle
        .files
        .iter()
        .find(|file| file.path == "response.json")
        .expect("response projection");
    let text = std::str::from_utf8(&response.bytes).expect("response UTF-8");
    assert!(text.contains("\"status\":\"rejected\""), "{text}");
    assert!(text.contains("\"lowered\":\"blocked\""), "{text}");
}

#[test]
fn checker_protocol_parameter_default_is_owned_before_default_resolution() {
    assert_eq!(
        self_checker_diagnostics(
            "protocol P { function f(value: Self = missingValue) -> Self }\n0\n",
        ),
        vec![(
            "TPZ5022".to_string(),
            "protocol method `P.f` cannot declare parameter defaults".to_string(),
            24,
            50,
        )],
    );
}

#[test]
fn checker_owns_user_protocol_declaration_surface() {
    let diagnostics = self_checker_diagnostics(concat!(
        "protocol Show {}\n",
        "type Existing = int\n",
        "protocol Existing {}\n",
        "protocol Pair<A, B> { function first(value: A) -> A }\n",
        "protocol Generic { function f<T>(value: Self) -> Self }\n",
        "protocol Empty { function make() -> int }\n",
        "protocol Return { function f(value: Self) }\n",
        "protocol Variadic { function f(...value: Self) -> Self }\n",
        "protocol Default { function f(value: Self = missingValue) -> Self }\n",
        "protocol Explicit<T> { function same(value: T, other: Self) -> T }\n",
        "protocol First { function f(value: int) -> int }\n",
        "protocol Methods {\n",
        "  function f(value: Self) -> Self\n",
        "  function f(value: Self) -> Self\n",
        "}\n",
        "protocol Twice {}\n",
        "protocol Twice {}\n",
        "function nested() -> int {\n",
        "  protocol Nested { function f(value: Self) -> Self }\n",
        "  0\n",
        "}\n",
        "0\n",
    ))
    .into_iter()
    .map(|(code, message, _, _)| (code, message))
    .collect::<Vec<_>>();
    assert_eq!(
            diagnostics,
            vec![
                (
                    "TPZ5008".to_string(),
                    "`Show` is a builtin protocol and cannot be redeclared".to_string(),
                ),
                (
                    "TPZ5022".to_string(),
                    "`Existing` is already a type and cannot also be a protocol".to_string(),
                ),
                (
                    "TPZ5022".to_string(),
                    "protocol `Pair` takes at most one conforming-type parameter; found 2"
                        .to_string(),
                ),
                (
                    "TPZ5022".to_string(),
                    "protocol method `Generic.f` cannot be generic".to_string(),
                ),
                (
                    "TPZ5022".to_string(),
                    "protocol method `Empty.make` must take the conforming value as its first parameter"
                        .to_string(),
                ),
                (
                    "TPZ5022".to_string(),
                    "protocol method `Return.f` requires an explicit return type (use `-> ()` for unit)"
                        .to_string(),
                ),
                (
                    "TPZ5022".to_string(),
                    "protocol method `Variadic.f` cannot be variadic".to_string(),
                ),
                (
                    "TPZ5022".to_string(),
                    "protocol method `Default.f` cannot declare parameter defaults".to_string(),
                ),
                (
                    "TPZ5022".to_string(),
                    "protocol method `First.f` must use `Self` or the protocol's type parameter as its first parameter type"
                        .to_string(),
                ),
                (
                    "TPZ5008".to_string(),
                    "method `f` is already declared in protocol `Methods`".to_string(),
                ),
                (
                    "TPZ5008".to_string(),
                    "protocol `Twice` is already declared".to_string(),
                ),
                (
                    "TPZ5022".to_string(),
                    "protocol declarations are module-top-level only".to_string(),
                ),
            ],
        );
}

#[test]
fn checker_consumes_user_protocol_signatures_at_static_calls() {
    const REJECTED_ARGUMENT: &str = concat!(
        "protocol Shift<T> {\n",
        "  function shift(value: T, delta: int) -> T\n",
        "}\n",
        "record Item { value: int }\n",
        "let moved = Shift.shift(Item { value: 1 }, \"wrong\")\n",
        "let deferred = Shift.shift([], \"wrong\")\n",
    );
    let rejected = self_checker_diagnostics(concat!(
        "protocol Merge<T> {\n",
        "  function merge(value: T, other: Self) -> T\n",
        "}\n",
        "record Item { value: int }\n",
        "let missingConformance = Merge.merge(Item { value: 1 }, \"wrong\")\n",
        "let extraAfterRejection = Merge.merge(Item { value: 2 }, \"wrong\", 3)\n",
        "let missingMethod = Merge.unknown(Item { value: 3 })\n",
        "let firstClass = Merge.merge\n",
    ))
    .into_iter()
    .map(|(code, message, _, _)| (code, message))
    .collect::<Vec<_>>();
    let arity = self_checker_diagnostics(concat!(
        "protocol Merge<T> {\n",
        "  function merge(value: T, other: Self) -> T\n",
        "}\n",
        "function incomplete<T: Merge>(value: T) -> T {\n",
        "  Merge.merge(value)\n",
        "}\n",
        "0\n",
    ))
    .into_iter()
    .map(|(code, message, _, _)| (code, message))
    .collect::<Vec<_>>();
    let bounded = typed_source(concat!(
        "protocol Merge<T> {\n",
        "  function merge(value: T, other: Self) -> T\n",
        "}\n",
        "function combine<T: Merge>(left: T, right: T) -> T {\n",
        "  Merge.merge(left, right)\n",
        "}\n",
        "0\n",
    ));
    let stage0_rejected_argument = stage0_checker_diagnostics(REJECTED_ARGUMENT)
        .into_iter()
        .map(|(code, message, _, _)| (code, message))
        .collect::<Vec<_>>();
    let self_rejected_argument = self_checker_diagnostics(REJECTED_ARGUMENT)
        .into_iter()
        .map(|(code, message, _, _)| (code, message))
        .collect::<Vec<_>>();
    assert_eq!(
        (
            rejected,
            arity,
            bounded.diagnostics.is_empty(),
            matches!(
                bounded.calls.last().map(|call| &call.result_type),
                Some(topaz_hir::SemanticType::Rigid { name, .. }) if name == "T"
            ),
            stage0_rejected_argument,
            self_rejected_argument,
        ),
        (
            vec![
                (
                    "TPZ5522".to_string(),
                    "`Item` does not conform to `Merge`".to_string(),
                ),
                (
                    "TPZ5522".to_string(),
                    "`Item` does not conform to `Merge`".to_string(),
                ),
                (
                    "TPZ5522".to_string(),
                    "protocol `Merge` has no method `unknown`".to_string(),
                ),
                ("TPZ5002".to_string(), "`Merge` is not bound".to_string(),),
            ],
            vec![(
                "TPZ5004".to_string(),
                "`Merge.merge` takes 2 arguments, found 1".to_string(),
            )],
            true,
            true,
            vec![
                (
                    "TPZ5522".to_string(),
                    "`Item` does not conform to `Shift`".to_string(),
                ),
                (
                    "TPZ5001".to_string(),
                    "expected `int`, found `string`".to_string(),
                ),
                (
                    "TPZ5001".to_string(),
                    "expected `int`, found `string`".to_string(),
                ),
            ],
            vec![
                (
                    "TPZ5522".to_string(),
                    "`Item` does not conform to `Shift`".to_string(),
                ),
                (
                    "TPZ5001".to_string(),
                    "expected `int`, found `string`".to_string(),
                ),
                (
                    "TPZ5001".to_string(),
                    "expected `int`, found `string`".to_string(),
                ),
            ],
        ),
    );
}

#[test]
fn checker_owns_manual_user_protocol_impl_formation_and_bodies() {
    const ACCEPTED: &str = concat!(
        "protocol Shift<T> { function shift(value: T, delta: int) -> T }\n",
        "record Point { value: int }\n",
        "impl Shift<Point> {\n",
        "  function shift(value: Point, delta: int) -> Point {\n",
        "    Point { value: value.value + delta }\n",
        "  }\n",
        "}\n",
        "record Pair { value: int }\n",
        "impl Eq<Pair> {\n",
        "  function equals(a: Pair, b: Pair) -> bool { false }\n",
        "}\n",
        "let unequal: bool = Eq.equals(Pair { value: 1 }, Pair { value: 1 })\n",
        "let moved: Point = Shift.shift(Point { value: 1 }, 2)\n",
    );
    const REJECTED: &str = concat!(
        "protocol Shift<T> { function shift(value: T, delta: int) -> T }\n",
        "record Point { value: int }\n",
        "impl Shift<Point> {\n",
        "  function shift(value: Point, delta: int) -> Point {\n",
        "    Point { value: value.value + delta }\n",
        "  }\n",
        "}\n",
        "let moved: Point = Shift.shift(Point { value: 1 }, 2)\n",
        "protocol Paint<T> { function paint(value: T) -> T }\n",
        "record Color { value: int }\n",
        "impl Paint<Color> {\n",
        "  function paint(value: Color) -> Color { \"wrong\" }\n",
        "}\n",
        "protocol Scale<T> { function scale(value: T, factor: int) -> T }\n",
        "record Size { value: int }\n",
        "impl Scale<Size> {\n",
        "  function scale(value: Size, factor: string) -> Size { value }\n",
        "}\n",
        "protocol Read<T> { function read(value: T) -> T }\n",
        "record Book { value: int }\n",
        "impl Read<Book> {\n",
        "  function other(value: Book) -> Book { value }\n",
        "}\n",
        "protocol Mark<T> { function mark(value: T) -> T }\n",
        "record Flag { value: int }\n",
        "impl Mark<Flag> { function mark(value: Flag) -> Flag { value } }\n",
        "impl Mark<Flag> { function mark(value: Flag) -> Flag { value } }\n",
        "record Rendered derives Show { name: string }\n",
        "impl Show<Rendered> {\n",
        "  function show(value: Rendered) -> string { value.name }\n",
        "}\n",
        "function nested() -> int {\n",
        "  impl Shift<Point> {\n",
        "    function shift(value: Point, delta: int) -> Point { value }\n",
        "  }\n",
        "  0\n",
        "}\n",
    );
    let accepted = typed_source(ACCEPTED);
    let rejected = typed_source(REJECTED);
    let lowered = preview_stage1_lowered(&InlineLoweringFixtureHost(ACCEPTED), lowering_request())
        .expect("manual protocol impl lowering");
    assert_eq!(
            (
                accepted.diagnostics,
                matches!(
                    accepted.calls.last().map(|call| &call.result_type),
                    Some(topaz_hir::SemanticType::NominalRecord { identity, .. })
                        if identity == "Point"
                ),
                accepted
                    .calls
                    .iter()
                    .filter_map(|call| call.target_identity.as_deref())
                    .filter(|identity| {
                        *identity == "builtin::Eq" || *identity == "builtin::Shift"
                    })
                    .collect::<Vec<_>>(),
                rejected
                    .diagnostics
                    .into_iter()
                    .map(|diagnostic| (diagnostic.code, diagnostic.message))
                    .collect::<Vec<_>>(),
                lowered.status,
                lowered.unsupported.len(),
                lowered
                    .operations
                    .iter()
                    .filter(|operation| operation.kind == "implementation")
                    .count(),
                lowered
                    .operations
                    .iter()
                    .filter(|operation| operation.kind == "function")
                    .count(),
                lowered
                    .operations
                    .iter()
                    .filter(|operation| operation.kind == "implementation")
                    .map(|operation| {
                        (
                            operation.detail.as_str(),
                            operation
                                .operands
                                .iter()
                                .filter_map(|operand| {
                                    lowered
                                        .operations
                                        .iter()
                                        .find(|candidate| candidate.id == *operand)
                                })
                                .filter(|candidate| candidate.kind == "function")
                                .map(|candidate| candidate.binding_name.as_str())
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>(),
                lowered
                    .operations
                    .iter()
                    .filter_map(|operation| {
                        if operation.kind == "expression/call"
                            && (operation.call_target == "builtin::Eq"
                                || operation.call_target == "builtin::Shift")
                        {
                            Some(operation.call_target.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
            ),
            (
                Vec::new(),
                true,
                vec!["builtin::Eq", "builtin::Shift"],
                vec![
                    (
                        "TPZ5022".to_string(),
                        "protocol implementation method `Scale.scale` must match the declared signature exactly; expected (Size, int) -> `Size`, found (Size, string) -> `Size`".to_string(),
                    ),
                    (
                        "TPZ5022".to_string(),
                        "protocol `Read` has no method `other`".to_string(),
                    ),
                    (
                        "TPZ5022".to_string(),
                        "`impl Read<Book>` is missing method `read`".to_string(),
                    ),
                    (
                        "TPZ5521".to_string(),
                        "`Flag` already conforms to `Mark` by a previous `impl`; a conformance must be unique".to_string(),
                    ),
                    (
                        "TPZ5521".to_string(),
                        "`Rendered` conformance to `Show` is already implemented manually; a conformance must be unique".to_string(),
                    ),
                    (
                        "TPZ5001".to_string(),
                        "expected `Color`, found `\"wrong\"`".to_string(),
                    ),
                    (
                        "TPZ5022".to_string(),
                        "impl declarations are module-top-level only".to_string(),
                    ),
                ],
                "completed".to_string(),
                0,
                2,
                2,
                vec![
                    ("Shift<Point>", vec!["shift"]),
                    ("Eq<Pair>", vec!["equals"]),
                ],
                vec!["builtin::Eq", "builtin::Shift"],
            ),
        );
}

#[test]
fn checker_owns_inherent_receiver_impls_and_lowered_dispatch_identity() {
    const ACCEPTED: &str = concat!(
        "record Point { value: int }\n",
        "impl Point {\n",
        "  export function shifted(self, delta: int = 1) -> Point {\n",
        "    Point { value: self.value + delta }\n",
        "  }\n",
        "}\n",
        "let moved: Point = Point { value: 40 }.shifted(delta: 2)\n",
        "let defaulted: Point = moved.shifted()\n",
        "let piped: Point = 2 |> defaulted.shifted()\n",
    );
    const REJECTED: &str = concat!(
        "record Broken { value: int, coordinate: int }\n",
        "impl Broken {\n",
        "  function noSelf() -> int { 0 }\n",
        "  function annotated(self: Broken) -> int { 0 }\n",
        "  function duplicate(self) -> int { 0 }\n",
        "  function duplicate(self) -> int { 1 }\n",
        "  function coordinate(self) -> int { 0 }\n",
        "  function length(self) -> int { 0 }\n",
        "  function wrong(self) -> int { \"bad\" }\n",
        "}\n",
        "record Box<T> { value: T }\n",
        "impl Box { function get(self) -> int { 0 } }\n",
        "impl int { function custom(self) -> int { 0 } }\n",
        "let broken = Broken { value: 1, coordinate: 2 }\n",
        "let method = broken.duplicate\n",
        "let maybe: Broken | null = broken\n",
        "let optional = maybe?.duplicate()\n",
    );
    let accepted = typed_source(ACCEPTED);
    let rejected = self_checker_diagnostics(REJECTED)
        .into_iter()
        .map(|(code, message, _, _)| (code, message))
        .collect::<Vec<_>>();
    let lowered = preview_stage1_lowered(&InlineLoweringFixtureHost(ACCEPTED), lowering_request())
        .expect("inherent receiver method lowering");
    let method_calls = accepted
        .calls
        .iter()
        .filter_map(|call| match &call.plan.callee {
            topaz_hir::CalleePlan::Member { method, .. }
            | topaz_hir::CalleePlan::Pipe {
                stage_method: Some(method),
            } if method == "shifted" => Some((
                call.target_identity.as_deref().unwrap_or(""),
                &call.result_type,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    let stage0_method_targets = stage0_typed_calls(ACCEPTED)
        .into_iter()
        .filter_map(|call| match call.plan.callee {
            topaz_hir::CalleePlan::Member { method, .. }
            | topaz_hir::CalleePlan::Pipe {
                stage_method: Some(method),
            } if method == "shifted" => Some(call.target_identity),
            _ => None,
        })
        .collect::<Vec<_>>();
    let lowered_calls = lowered
        .operations
        .iter()
        .filter(|operation| {
            operation.call_method == "shifted" || operation.call_stage_method == "shifted"
        })
        .map(|operation| operation.call_target.as_str())
        .collect::<Vec<_>>();
    let imported = preview_typed(
        &ReceiverMethodImportFixtureHost,
        topaz_kernel::KernelRequest::checked(
            "root/main.tpz",
            Some("root"),
            LangVersion::CURRENT,
            topaz_kernel::PackageFacts::standalone(),
        )
        .with_terminal_phase(topaz_kernel::TerminalPhase::Typed),
    )
    .expect("imported receiver method preview");
    let imported_calls = imported
        .calls
        .iter()
        .filter_map(|call| match &call.plan.callee {
            topaz_hir::CalleePlan::Member { method, .. }
            | topaz_hir::CalleePlan::Pipe {
                stage_method: Some(method),
            } if method == "shifted" => Some((method.clone(), call.target_identity.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    let stage0_imported_calls = stage0_resolved_unit(
        &ReceiverMethodImportFixtureHost,
        topaz_kernel::KernelRequest::checked(
            "root/main.tpz",
            Some("root"),
            LangVersion::CURRENT,
            topaz_kernel::PackageFacts::standalone(),
        )
        .with_terminal_phase(topaz_kernel::TerminalPhase::Typed),
    )
    .checked
    .and_then(|checked| checked.typed_hir)
    .expect("Stage 0 imported receiver method preview")
    .calls
    .into_iter()
    .filter_map(|call| match call.plan.callee {
        topaz_hir::CalleePlan::Member { method, .. }
        | topaz_hir::CalleePlan::Pipe {
            stage_method: Some(method),
        } if method == "shifted" => Some((method, call.target_identity)),
        _ => None,
    })
    .collect::<Vec<_>>();
    assert_eq!(
            (
                accepted.diagnostics,
                method_calls,
                stage0_method_targets,
                rejected,
                lowered.status,
                lowered.unsupported,
                lowered
                    .operations
                    .iter()
                    .filter(|operation| operation.kind == "implementation")
                    .map(|operation| {
                        (
                            operation.detail.as_str(),
                            operation
                                .operands
                                .iter()
                                .filter_map(|operand| {
                                    lowered
                                        .operations
                                        .iter()
                                        .find(|candidate| candidate.id == *operand)
                                })
                                .filter(|candidate| candidate.kind == "function")
                                .map(|candidate| candidate.binding_name.as_str())
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>(),
                lowered_calls,
                imported
                    .diagnostics
                    .into_iter()
                    .map(|diagnostic| (diagnostic.code, diagnostic.message))
                    .collect::<Vec<_>>(),
                imported_calls,
                stage0_imported_calls,
            ),
            (
                Vec::new(),
                vec![
                    (
                        "main::Point",
                        &topaz_hir::SemanticType::NominalRecord {
                            identity: "Point".to_string(),
                            arguments: Vec::new(),
                        },
                    ),
                    (
                        "main::Point",
                        &topaz_hir::SemanticType::NominalRecord {
                            identity: "Point".to_string(),
                            arguments: Vec::new(),
                        },
                    ),
                    (
                        "main::Point",
                        &topaz_hir::SemanticType::NominalRecord {
                            identity: "Point".to_string(),
                            arguments: Vec::new(),
                        },
                    ),
                ],
                vec![
                    Some("main::Point".to_string()),
                    Some("main::Point".to_string()),
                    Some("main::Point".to_string()),
                ],
                vec![
                    (
                        "TPZ5022".to_string(),
                        "method `noSelf` on `Broken` must take `self` as its first parameter"
                            .to_string(),
                    ),
                    (
                        "TPZ5022".to_string(),
                        "method `annotated` on `Broken` must take bare `self` as its first parameter (no type annotation, default, or variadic marker)"
                            .to_string(),
                    ),
                    (
                        "TPZ5008".to_string(),
                        "method `duplicate` is already defined for `Broken`".to_string(),
                    ),
                    (
                        "TPZ5022".to_string(),
                        "method `coordinate` on `Broken` collides with a field of the same name; rename the method (a field shadows a method)"
                            .to_string(),
                    ),
                    (
                        "TPZ5022".to_string(),
                        "method name `length` on `Broken` collides with a builtin member; choose another name"
                            .to_string(),
                    ),
                    (
                        "TPZ5022".to_string(),
                        "cannot define receiver methods on generic nominal `Box` yet; generic `impl` binders are not defined"
                            .to_string(),
                    ),
                    (
                        "TPZ5022".to_string(),
                        "cannot define methods on `int` (a builtin type); `impl` is only allowed on an own-module record, enum, or newtype"
                            .to_string(),
                    ),
                    (
                        "TPZ5001".to_string(),
                        "expected `int`, found `string`".to_string(),
                    ),
                    (
                        "TPZ5006".to_string(),
                        "`Broken` has no member named `duplicate`".to_string(),
                    ),
                    (
                        "TPZ5006".to_string(),
                        "record `Broken` has no field `duplicate`".to_string(),
                    ),
                ],
                "completed".to_string(),
                Vec::new(),
                vec![("Point", vec!["shifted"])],
                vec!["main::Point", "main::Point", "main::Point"],
                Vec::new(),
                vec![
                    ("shifted".to_string(), Some("model::Point".to_string())),
                    ("shifted".to_string(), Some("model::Point".to_string())),
                ],
                vec![
                    ("shifted".to_string(), Some("model::Point".to_string())),
                    ("shifted".to_string(), Some("model::Point".to_string())),
                ],
            ),
        );
}

#[test]
fn checker_unbound_name_suggestions_match_stage0() {
    let source = concat!(
        "function localValueSuggestion() -> () {\n",
        "  let length = 5\n  let value = lenght\n}\n",
        "function callableSuggestion() -> () {\n",
        "  let myfunc = (value: int) => value\n  let result = myfnc(1)\n}\n",
        "function nonCallableSuggestion() -> () {\n",
        "  let myval = 5\n  myvl()\n}\n",
        "function shadowedBuiltinSuggestion() -> () {\n",
        "  let print = 1\n  prnt(\"x\")\n}\n",
        "function builtinSuggestion() -> () { prnt(\"x\") }\n",
        "function constantSuggestion() -> () { let value = Noen }\n",
        "function unrelatedName() -> () { let value = qqqqqq }\n",
        "function stableTie() -> () {\n",
        "  let cast = 1\n  let cart = 2\n  let value = catt\n}\n",
        "function stableCalleeTie() -> () {\n",
        "  let dast = () => ()\n  let dart = () => ()\n  datt()\n}\n",
    );
    let self_diagnostics = self_checker_diagnostics(source);
    assert_eq!(self_diagnostics, stage0_unit_checker_diagnostics(source));
    for expected in [
        "`lenght` is not bound; did you mean `length`?",
        "`myfnc` is not bound; did you mean `myfunc`?",
        "`myvl` is not bound",
        "`prnt` is not bound",
        "`prnt` is not bound; did you mean `print`?",
        "`Noen` is not bound; did you mean `None`?",
        "`qqqqqq` is not bound",
        "`catt` is not bound; did you mean `cart`?",
        "`datt` is not bound; did you mean `dart`?",
    ] {
        assert!(
            self_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.1 == expected),
            "{expected}: {self_diagnostics:?}",
        );
    }
}

#[test]
fn checker_call_binding_uses_names_defaults_and_variadic_rules() {
    let accepted = concat!(
        "function add(left: int, right: int = 1) -> int { left + right }\n",
        "function keep<T>(value: T) -> T { value }\n",
        "function identity(value: int) -> int { value }\n",
        "function render(value: string) -> string { value }\n",
        "function sum(left: int, right: int) -> int { left + right }\n",
        "let answer = add(41)\n",
        "let piped = 41 |> add()\n",
        "let named = 40 |> add(right: 2)\n",
        "let inferred: string = \"ok\" |> keep()\n",
        "let explicit: string = \"ok\" |> keep<string>()\n",
        "let block: int = 5 |> identity({ _ })\n",
        "let branch: int = 6 |> identity(if true { _ } else { 0 })\n",
        "let text: string = 7 |> render(\"value {_}\")\n",
        "let nested: int = 10 |> sum(100 |> sum(_, 1))\n",
        "function projectedDirect<T>(value: T) { let result = value.run(41) }\n",
        "function projectedOptional<T>(value: Option<T>) { let result = value?.run(41) }\n",
        "function projectedNullable<T>(value: T | null) { let result = value?.run(41) }\n",
    );
    let accepted_self = self_checker_diagnostics(accepted);
    assert_eq!(accepted_self, stage0_checker_diagnostics(accepted));
    assert!(accepted_self.is_empty());

    let projected_pipeline = concat!(
        "type Runner = { run: (int) -> int, join: (int, int) -> int }\n",
        "function invokeDirect<T>(value: Runner | T) { let result = value.run(1) }\n",
        "function invokeDirectOptional<T>(value: Option<Runner | T>) {\n",
        "  let result = value?.run(1)\n",
        "}\n",
        "function invoke<T>(value: Runner | T) { let result = 1 |> value.run() }\n",
        "function invokeOptional<T>(value: Option<Runner | T>) {\n",
        "  let result = 1 |> value?.run()\n",
        "}\n",
        "function invokeWithArgument<T>(value: Runner | T, extra: int) {\n",
        "  let result = 1 |> value.join(extra)\n",
        "}\n",
        "function plus(left: int, right: int) -> int { left + right }\n",
        "function invokeValuePipe(extra: int) { let result = 1 |> plus(extra) }\n",
    );
    let self_calls = typed_source(projected_pipeline).calls;
    let stage0_calls = stage0_typed_calls(projected_pipeline);
    assert_eq!(self_calls, stage0_calls);
    assert_eq!(
        stage0_calls
            .iter()
            .map(|call| call.target_identity.as_deref())
            .collect::<Vec<_>>(),
        [None, None, None, None, None, Some("main::plus")]
    );

    let std_dom_constructors = concat!(
        "import std.dom { Html, Command, WebAppCommand }\n",
        "enum Msg { Ready }\n",
        "let html: Html<Msg> = Html.Text(\"ready\")\n",
        "let command: Command<Msg> = Command.Dispatch(Msg.Ready)\n",
        "let web: WebAppCommand<Msg> = WebAppCommand.Dom(command)\n",
    );
    let msg = topaz_hir::SemanticType::Enum {
        identity: "Msg".to_string(),
        arguments: Vec::new(),
    };
    let nominal = |identity: &str| topaz_hir::SemanticType::Enum {
        identity: identity.to_string(),
        arguments: vec![msg.clone()],
    };
    let html = nominal("std.dom::Html");
    let command = nominal("std.dom::Command");
    let web = nominal("std.dom::WebAppCommand");
    let expected_std_dom_calls = vec![
        (
            topaz_hir::SemanticType::Function {
                parameters: vec![topaz_hir::SemanticType::Primitive(
                    topaz_hir::SemanticPrimitive::String,
                )],
                variadic: None,
                result: Box::new(html.clone()),
            },
            html,
            None,
            false,
        ),
        (
            topaz_hir::SemanticType::Function {
                parameters: vec![msg],
                variadic: None,
                result: Box::new(command.clone()),
            },
            command.clone(),
            None,
            false,
        ),
        (
            topaz_hir::SemanticType::Function {
                parameters: vec![command],
                variadic: None,
                result: Box::new(web.clone()),
            },
            web,
            None,
            false,
        ),
    ];
    let project_main_calls = |calls: Vec<topaz_hir::TypedCall>| {
        calls
            .into_iter()
            .filter(|call| call.module == "main")
            .map(|call| {
                (
                    call.callee_type,
                    call.result_type,
                    call.target_identity,
                    call.ambient,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        project_main_calls(stage0_typed_calls(std_dom_constructors)),
        expected_std_dom_calls,
    );
    let self_std_dom = typed_source(std_dom_constructors);
    assert!(
        self_std_dom.resolved.diagnostics.is_empty(),
        "self-host std.dom constructors did not resolve: {:?}",
        self_std_dom.resolved.diagnostics,
    );
    assert!(
        self_std_dom.diagnostics.is_empty(),
        "self-host std.dom constructors did not check: {:?}",
        self_std_dom.diagnostics,
    );
    assert_eq!(
        project_main_calls(self_std_dom.calls),
        expected_std_dom_calls,
    );

    let cases = [
        (
            concat!(
                "function add(left: int, right: int) -> int { left + right }\n",
                "let answer = add(1)\n",
            ),
            "TPZ5004",
            "this call needs 2 arguments, found 1",
        ),
        (
            concat!(
                "function take(value: int, other: int = 0) -> int { value + other }\n",
                "let answer = take(value: \"no\")\n",
            ),
            "TPZ5001",
            "expected `int`, found `string`",
        ),
        (
            concat!(
                "function take(value: int, other: int) -> int { value + other }\n",
                "let answer = take(value: 1, 2)\n",
            ),
            "TPZ5004",
            "positional arguments may not follow named arguments",
        ),
        (
            concat!(
                "function take(value: int) -> int { value }\n",
                "let answer = take(...[1])\n",
            ),
            "TPZ5004",
            "spread arguments require a variadic parameter",
        ),
        (
            "let value = 1\nlet answer = value()\n",
            "TPZ5005",
            "`int` is not callable",
        ),
        (
            concat!(
                "function take(value: int) -> int { value }\n",
                "let answer = take(1, 2)\n",
            ),
            "TPZ5004",
            "this call needs 1 argument, found 2",
        ),
        (
            concat!(
                "function apply(f: (int) -> int) -> int { f(1, 2) }\n",
                "let answer = apply((value: int) => value)\n",
            ),
            "TPZ5004",
            "this call needs 1 argument, found 2",
        ),
        (
            concat!(
                "function take(value: int) -> int { value }\n",
                "let answer = \"no\" |> take()\n",
            ),
            "TPZ5001",
            "expected `int`, found `string`",
        ),
        (
            concat!(
                "function take(value: int) -> int { value }\n",
                "let answer = 1 |> take(value: 2)\n",
            ),
            "TPZ5004",
            "`value` is already supplied by the pipeline (§11)",
        ),
        ("let answer = 1 |> 2\n", "TPZ5005", "`2` is not callable"),
        (
            "function invoke<T>(value: Option<{ run: int } | T>) { let result = value?.run(41) }\n",
            "TPZ5005",
            "`int` is not callable",
        ),
        (
            "function invoke<T>(value: Option<Array<int> | T>) { let result = value?.get(\"bad\") }\n",
            "TPZ5001",
            "expected `int`, found `string`",
        ),
        (
            "function invoke<T>(value: Option<Array<int> | T>) { let result = value?.get() }\n",
            "TPZ5004",
            "this call needs 1 argument, found 0",
        ),
        (
            concat!("function zero() -> int { 0 }\n", "let answer = 1 |> zero\n",),
            "TPZ5004",
            "a pipeline stage takes the piped value as its only argument (§11)",
        ),
        (
            concat!(
                "function zero() -> int { 0 }\n",
                "let answer = 1 |> zero()\n",
            ),
            "TPZ5004",
            "the pipeline inserts the piped value, but this function takes no parameters",
        ),
        (
            "let answer: int = _\n",
            "TPZ5001",
            "a placeholder `_` is valid only inside a pipeline stage (§11)",
        ),
        (
            concat!(
                "function echo(value: int) -> int { value }\n",
                "let answer = echo |> _(1)\n",
            ),
            "TPZ5001",
            "a placeholder `_` is valid only in a pipeline stage's argument list (§11)",
        ),
    ];
    let mut differences = Vec::new();
    for (source, code, message) in cases {
        let self_diagnostics = self_checker_diagnostics(source);
        assert!(
            self_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.0 == code && diagnostic.1 == message),
            "{source}: {self_diagnostics:?}",
        );
        let stage0_diagnostics = stage0_checker_diagnostics(source);
        if self_diagnostics != stage0_diagnostics {
            differences.push(format!(
                "{source}\nself={self_diagnostics:?}\nstage0={stage0_diagnostics:?}"
            ));
        }
    }
    for source in [
        concat!(
            "function add(left: int, right: int) -> int { left + right }\n",
            "let value: string = add(1)\n",
        ),
        concat!(
            "function take(value: int) -> int { value }\n",
            "let value: string = take(1, 2)\n",
        ),
        concat!(
            "function take(value: int) -> int { value }\n",
            "let value: string = take(missing: 1, ...[2])\n",
        ),
        concat!(
            "function take(value: int, other: int) -> int { value + other }\n",
            "let value = take(value: \"bad\", 2)\n",
        ),
        concat!(
            "function invoke(f: (int) -> int) -> () {\n",
            "  let value: string = f(value: \"x\")\n",
            "}\n",
        ),
        concat!(
            "function invoke(f: (int) -> int) -> () {\n",
            "  let value = f<int>(value: 1)\n",
            "}\n",
        ),
        concat!(
            "function invoke(f: (int) -> int) -> () {\n",
            "  let value: string = f(...[\"x\"])\n",
            "}\n",
        ),
        concat!(
            "function invoke(f: (int) -> int) -> () {\n",
            "  let value: string = f()\n",
            "}\n",
        ),
        concat!(
            "function invoke(f: (int) -> int) -> () {\n",
            "  let value: string = f(1, 2)\n",
            "}\n",
        ),
        concat!(
            "function invoke(f: (int) -> int) -> () {\n",
            "  let value: string = f(value: 1, ...[2])\n",
            "}\n",
        ),
        concat!(
            "function invoke(f: (...int) -> int) -> () {\n",
            "  let value: string = f(value: 1, ...[2])\n",
            "}\n",
        ),
        concat!(
            "function gather(first: int, ...rest: int) -> int { first }\n",
            "let value: string = gather(...[1], first: 2)\n",
        ),
        concat!(
            "function gather(first: int, ...rest: int) -> int { first }\n",
            "let value: string = gather(...[1])\n",
        ),
        concat!(
            "function gather(first: int, ...rest: int) -> int { first }\n",
            "let value: string = gather(...[1], 2, first: 3)\n",
        ),
        concat!(
            "function gather(first: int, ...rest: int) -> int { first }\n",
            "let value: string = gather(first: 1, ...[2])\n",
        ),
        concat!(
            "function gather(tag: string = \"x\", ...rest: int) -> int { rest.length }\n",
            "let value: string = gather(tag: \"y\", ...[1])\n",
        ),
        "let callee = 1\nlet value = callee(...[2])\n",
        concat!(
            "function zero() -> int { 0 }\n",
            "let value: string = 1 |> zero()\n",
        ),
        concat!(
            "function invoke<T>(value: Option<T>) -> Option<int> {\n",
            "  return value?.run(41)\n",
            "}\n",
        ),
        concat!(
            "function invoke<T>(value: Option<{ other: int } | T>) {\n",
            "  let result = value?.run(41)\n",
            "}\n",
        ),
        concat!(
            "function invoke<T>(value: { other: int } | T) {\n",
            "  let result = value.run(41)\n",
            "}\n",
        ),
        concat!(
            "type Numeric = { run: (int) -> int }\n",
            "type Textual = { run: (string) -> string }\n",
            "function invoke<T>(value: Option<Numeric | Textual | T>) {\n",
            "  let result = value?.run(true)\n",
            "}\n",
        ),
        concat!(
            "function invoke<T>(value: Option<Array<int> | T>) {\n",
            "  let result = value?.get(j: 0)\n",
            "}\n",
        ),
        concat!(
            "function invoke<T>(value: { run: () -> string } | T) {\n",
            "  let result = 1 |> value.run()\n",
            "}\n",
        ),
        concat!(
            "function invoke<T>(value: Array<int> | T) {\n",
            "  let result = value.push(1)\n",
            "}\n",
        ),
    ] {
        let self_diagnostics = self_checker_diagnostics(source);
        let stage0_diagnostics = stage0_checker_diagnostics(source);
        if self_diagnostics != stage0_diagnostics {
            differences.push(format!(
                "{source}\nself={self_diagnostics:?}\nstage0={stage0_diagnostics:?}"
            ));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n\n"));
}

#[test]
fn checker_builtin_call_signatures_match_stage0_named_and_default_planning() {
    let accepted = concat!(
        "function addPair(left: int, right: int) -> int { left + right }\n",
        "let buffer = ByteBuffer.allocate(value: 7, length: 4)\n",
        "Test.assert(message: \"ok\", condition: true)\n",
        "let compressed = Codec.zstdCompress(level: 3, bytes: Bytes.empty())\n",
        "let rounded = Decimal.fromInt(n: 7).round(mode: RoundingMode.Down, scale: 0)\n",
        "let total = [1, 2].reduce(f: addPair, initial: 0)\n",
        "let date = Date.fromYmd(day: 11, month: 8, year: 2026)\n",
        "true |> Test.assert(message: \"piped\")\n",
        "2026 |> Date.fromYmd(month: 8, day: 11)\n",
    );
    let accepted_preview = self_checker_diagnostics(accepted);
    assert_eq!(accepted_preview, stage0_checker_diagnostics(accepted));
    assert!(accepted_preview.is_empty());

    let catalog = concat!(
        "function increment(value: int) -> int { value + 1 }\n",
        "function addPair(left: int, right: int) -> int { left + right }\n",
        "function exercise(\n",
        "  option: Option<int>,\n",
        "  result: Result<int, string>,\n",
        "  values: Map<string, int>,\n",
        "  members: Set<int>,\n",
        "  bytes: Bytes,\n",
        "  buffer: ByteBuffer,\n",
        "  file: File,\n",
        "  path: Path,\n",
        "  regex: Regex,\n",
        "  url: URL,\n",
        "  date: Date,\n",
        "  big: BigInt,\n",
        "  decimal: Decimal,\n",
        "  json: JSONValue,\n",
        ") -> () {\n",
        "  let lower = 3.atLeast(min: 1)\n",
        "  let replaced = \"aba\".replace(new: \"c\", old: \"a\")\n",
        "  let fallback = option.okOr(error: \"missing\")\n",
        "  let mapped = result.map(f: increment)\n",
        "  let sum = [1, 2].reduce(f: addPair, initial: 0)\n",
        "  let found = values.getOr(default: 0, k: \"key\")\n",
        "  let union = members.union(other: members)\n",
        "  let piece = bytes.slice(end: 2, start: 0)\n",
        "  let byte = buffer.get(index: 0)\n",
        "  let written = file.write(s: \"text\")\n",
        "  let child = path.join(child: \"name\")\n",
        "  let changed = regex.replaceAll(replacement: \"x\", text: \"y\")\n",
        "  let shownUrl = url.toString()\n",
        "  let tomorrow = date.addDays(days: 1)\n",
        "  let quotient = big.div(other: big)\n",
        "  let divided = decimal.div(mode: RoundingMode.Down, scale: 0, other: decimal)\n",
        "  let childJson = json.get(key: \"key\")\n",
        "}\n",
        "let parsed = toInt(text: \"1\")\n",
        "let maximum = Math.max(b: 2.0, a: 1.0)\n",
        "let combined = Bytes.concat(b: Bytes.empty(), a: Bytes.empty())\n",
        "let decoded = Encoding.hexDecode(text: \"00\")\n",
        "let digest = Hash.hmacSha256(message: Bytes.empty(), key: Bytes.empty())\n",
        "let option = Cli.option(name: \"flag\", args: [])\n",
        "let written = FS.writeBytes(bytes: Bytes.empty(), path: \"file\")\n",
        "let json = JSON.parse(text: \"null\")\n",
        "let url = URL.parse(text: \"https://example.com\")\n",
        "let big = BigInt.parse(radix: 10, text: \"7\")\n",
    );
    let catalog_preview = self_checker_diagnostics(catalog);
    assert_eq!(catalog_preview, stage0_checker_diagnostics(catalog));
    assert!(catalog_preview.is_empty());

    for source in [
        "let buffer = ByteBuffer.allocate(value: 7)\n",
        "Test.assert(message: \"no condition\")\n",
        "let rounded = Decimal.fromInt(n: 7).round(mode: RoundingMode.Down)\n",
        "let date = Date.fromYmd(year: 2026, month: 8, hour: 1)\n",
        concat!(
            "function identity(value: int) -> int { value }\n",
            "let value = identity<int>(1)\n",
        ),
    ] {
        assert_eq!(
            self_checker_diagnostics(source),
            stage0_checker_diagnostics(source),
            "{source}",
        );
    }

    let positional_protocol = concat!(
        "record User derives Show { name: string }\n",
        "let user = User { name: \"Ada\" }\n",
        "let shown = Show.show(value: user)\n",
    );
    assert_eq!(
        self_checker_diagnostics(positional_protocol),
        stage0_checker_diagnostics(positional_protocol),
    );
}

#[test]
fn checker_builtin_identifier_and_static_member_catalog_matches_stage0() {
    for source in [
        "let builtin = open\nlet result = builtin()\n",
        "let builtin = Some\nlet result = builtin()\n",
        "let builtin = Ok\nlet result = builtin()\n",
        "let builtin = Err\nlet result = builtin()\n",
        "let builtin = map\nlet result = builtin()\n",
        "let builtin = filter\nlet result = builtin()\n",
        "let builtin = reduce\nlet result = builtin()\n",
    ] {
        assert_eq!(
            self_checker_diagnostics(source),
            stage0_unit_checker_diagnostics(source),
            "{source}",
        );
    }

    let namespace_heads = [
        "Array",
        "Set",
        "Bytes",
        "ByteBuffer",
        "Map",
        "Math",
        "Codec",
        "Hash",
        "FS",
        "Encoding",
        "Cli",
        "Path",
        "Regex",
        "CSV",
        "TOML",
        "JSON",
        "URL",
        "Date",
        "BigInt",
        "Decimal",
        "RoundingMode",
        "Test",
        "Show",
        "Eq",
        "Order",
    ];
    let namespace_source = namespace_heads
        .iter()
        .enumerate()
        .map(|(index, name)| format!("let namespace{index} = {name}\n"))
        .collect::<String>();
    let namespace_source = namespace_source.as_str();
    let namespace_self = self_checker_diagnostics(namespace_source);
    assert_eq!(
        namespace_self,
        stage0_unit_checker_diagnostics(namespace_source),
    );
    assert_eq!(namespace_self.len(), namespace_heads.len());

    let mut static_source = String::new();
    let mut static_index = 0;
    for namespace in topaz_check::builtins::STATIC_NAMESPACE_NAMES {
        for member in topaz_check::builtins::static_member_names(namespace) {
            static_source.push_str(&format!(
                "let staticMember{static_index} = {namespace}.{member}\n"
            ));
            static_index += 1;
        }
    }
    for (index, member) in [
        "Down",
        "Up",
        "TowardZero",
        "AwayFromZero",
        "HalfEven",
        "HalfUp",
    ]
    .iter()
    .enumerate()
    {
        static_source.push_str(&format!(
            "let roundingMode{index} = RoundingMode.{member}\n"
        ));
    }
    let static_source = static_source.as_str();
    let static_self = self_checker_diagnostics(static_source);
    assert_eq!(static_self, stage0_checker_diagnostics(static_source));
    assert!(static_self.is_empty(), "{static_self:?}");

    let shadowed_builtins = concat!(
        "record MathSurface { floor: (int) -> int }\n",
        "function floorInt(value: int) -> int { value }\n",
        "function readFloor(Math: MathSurface) -> int { Math.floor(7) }\n",
        "function callMap(map: (int, string) -> int) -> int { map(7, \"x\") }\n",
        "let floor = readFloor(MathSurface { floor: floorInt })\n",
    );
    let shadowed_self = self_checker_diagnostics(shadowed_builtins);
    assert_eq!(shadowed_self, stage0_checker_diagnostics(shadowed_builtins),);
    assert!(shadowed_self.is_empty(), "{shadowed_self:?}");

    let shadowed_constructor =
        "function callOk(Ok: (int) -> int) -> Result<int, string> { Ok(7) }\n";
    assert_eq!(
        self_checker_diagnostics(shadowed_constructor),
        stage0_checker_diagnostics(shadowed_constructor),
    );
}
