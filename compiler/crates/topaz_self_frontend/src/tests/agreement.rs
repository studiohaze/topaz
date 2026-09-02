use super::*;

#[test]
fn checker_optional_access_matches_stage0_unwrapping_calls_and_diagnostics() {
    let accepted = concat!(
        "let user: Option<{ name: string, profile: Option<{ city: string }> }> = None\n",
        "let name: Option<string> = user?.name\n",
        "let city: Option<string> = user?.profile?.city\n",
        "let nullable: { name: string } | null = null\n",
        "let nullableName: string | null = nullable?.name\n",
        "let text: string | null = \"Topaz\"\n",
        "let scalars: Array<string> | null = text?.scalars()\n",
        "let xs: Option<Array<int>> = Some([1, 2])\n",
        "let first: Option<int> = xs?.get(i: 0)\n",
        "let length: Option<int> = xs?.length\n",
        "xs?.push(x: 3)\n",
        "let piped: Option<int> = 0 |> xs?.get()\n",
    );
    let accepted_self = self_checker_diagnostics(accepted);
    assert_eq!(accepted_self, stage0_checker_diagnostics(accepted));
    assert!(accepted_self.is_empty());

    for source in [
        concat!(
            "let user: { name: string } = { name: \"Ada\" }\n",
            "let name = user?.name\n",
        ),
        concat!(
            "let user: Option<{ name: string }> = None\n",
            "let email = user?.email\n",
        ),
        concat!(
            "let xs: Option<Array<int>> = Some([1])\n",
            "let length = xs?.length()\n",
        ),
        concat!(
            "let xs: Option<Array<int>> = Some([1])\n",
            "let first = xs?.get<int>(0)\n",
        ),
    ] {
        let self_diagnostics = self_checker_diagnostics(source);
        assert_eq!(
            self_diagnostics,
            stage0_checker_diagnostics(source),
            "{source}",
        );
        assert!(!self_diagnostics.is_empty(), "{source}");
    }
}

#[test]
fn checker_operator_index_iterable_and_capability_gates_match_stage0() {
    const BYTE_BUFFER_TEXT: &str = concat!(
        "record Packet { body: ByteBuffer }\n",
        "enum Message { Data(Packet) }\n",
        "newtype Envelope = Message\n",
        "\n",
        "function render(\n",
        "  buffer: ByteBuffer,\n",
        "  packet: Packet,\n",
        "  message: Message,\n",
        "  envelope: Envelope,\n",
        ") -> () {\n",
        "  let direct = \"{buffer}\"\n",
        "  let nestedRecord = \"{packet}\"\n",
        "  let nestedEnum = \"{message}\"\n",
        "  let nestedNewtype = sql\"{envelope}\"\n",
        "}\n",
    );
    let accepted = concat!(
        "record OrderedKey { id: int, name: string }\n",
        "record DerivedValue derives Eq, Order, Show, JSON { id: int, name: string }\n",
        "function specialOperators(big: BigInt, decimal: Decimal, bytesA: Bytes, bytesB: Bytes) -> bool {\n",
        "  let negatedBig = -big\n",
        "  let addedBig = big + big\n",
        "  let multipliedDecimal = decimal * decimal\n",
        "  bytesA < bytesB\n",
        "}\n",
        "function visitOpaque<T>(values: T) -> () { for ignored in values { let one = 1 }\n  ()\n}\n",
        "function encodeBound<T: JSON>(value: T) -> Result<string, string> { JSON.stringify(value) }\n",
        "let integerSum = 1 + 2\n",
        "let floatProduct = 1.5 * 2.0\n",
        "let textSum = \"a\" + \"b\"\n",
        "let negated = !true\n",
        "let xs = [1, 2, 3]\n",
        "let spreadXs = [0, ...xs, 4]\n",
        "let first: int = xs[0]\n",
        "let maybeXs: Array<int> | string = xs\n",
        "let maybeFirst = maybeXs[0]\n",
        "for value in xs { let copy: int = value }\n",
        "let member = 2 in xs\n",
        "let rangeMember = 2 in 0..3\n",
        "record Point { x: int, label: string }\n",
        "let pointA = Point { x: 1, label: \"a\" }\n",
        "let pointB = Point { x: 2, label: \"b\" }\n",
        "let equalPoints = pointA == pointB\n",
        "let orderedPoints = pointA < pointB\n",
        "let keySet: Set<int> = Set.of(1, 2)\n",
        "let keyMap: Map<string, int> = Map.new()\n",
        "let recordKeys: Set<OrderedKey> = Set.of()\n",
        "let sortedInts = [3, 1, 2].sorted()\n",
        "let optionText: Option<string> = None\n",
        "let defaultText = optionText ?? \"default\"\n",
        "let encoded: Result<string, string> = JSON.stringify([1, 2])\n",
        "let decoded: Result<Array<int>, string> = JSON.parseAs<Array<int>>(\"[1,2]\")\n",
        "let derivedEncoded = encodeBound(DerivedValue { id: 1, name: \"ok\" })\n",
    );
    let byte_buffer_message =
        "`ByteBuffer` values cannot be interpolated; snapshot to `Bytes` and encode explicitly";
    assert_eq!(
        (
            self_checker_diagnostics(accepted),
            stage0_checker_diagnostics(accepted),
            stage0_checker_diagnostics(BYTE_BUFFER_TEXT),
            self_checker_diagnostics(BYTE_BUFFER_TEXT),
        ),
        (
            Vec::new(),
            Vec::new(),
            vec![
                (
                    "TPZ5001".to_string(),
                    byte_buffer_message.to_string(),
                    219,
                    225,
                ),
                (
                    "TPZ5001".to_string(),
                    byte_buffer_message.to_string(),
                    251,
                    257,
                ),
                (
                    "TPZ5001".to_string(),
                    byte_buffer_message.to_string(),
                    281,
                    288,
                ),
                (
                    "TPZ5001".to_string(),
                    byte_buffer_message.to_string(),
                    318,
                    326,
                ),
            ],
            vec![
                (
                    "TPZ5001".to_string(),
                    byte_buffer_message.to_string(),
                    219,
                    225,
                ),
                (
                    "TPZ5001".to_string(),
                    byte_buffer_message.to_string(),
                    251,
                    257,
                ),
                (
                    "TPZ5001".to_string(),
                    byte_buffer_message.to_string(),
                    281,
                    288,
                ),
                (
                    "TPZ5001".to_string(),
                    byte_buffer_message.to_string(),
                    318,
                    326,
                ),
            ],
        ),
    );

    let rejected = concat!(
        "let badAddText = 1 + \"two\"\n",
        "let badAddFloat = 1 + 2.5\n",
        "let badLogic = true && 1\n",
        "let badUnary = -\"flip\"\n",
        "let badNot = !1\n",
        "let badRemainder = 1.5 % 1.0\n",
        "let badMultiply = 1 * \"two\"\n",
        "let badOrder = 1 < \"two\"\n",
        "let badEquality = 1 == \"one\"\n",
        "let indexed = [1, 2, 3]\n",
        "let badArrayIndex = indexed[\"zero\"]\n",
        "let mut indexedMap: Map<string, int> = Map.new()\n",
        "let badMapIndex = indexedMap[\"a\"]\n",
        "let indexedText = \"abc\"\n",
        "let badTextIndex = indexedText[0]\n",
        "let indexedSet = Set.of(1, 2)\n",
        "let badSetIndex = indexedSet[0]\n",
        "let badSpread = [...indexedSet]\n",
        "for scalar in 1 { scalar }\n",
        "let badComprehension = map { for scalar in 1 => scalar: scalar }\n",
        "let badMapMembership = 1 in Map.new<string, int>()\n",
        "let badTextMembership = \"a\" in \"abc\"\n",
        "let badElementMembership = \"a\" in [1, 2]\n",
        "let badCoalesce = 1 ?? 2\n",
        "let stringOption: Option<string> = None\n",
        "let badCoalesceRight = stringOption ?? 1\n",
        "let badMapFunction = map(1, (value) => value)\n",
        "let badKeyAnnotation: Set<(int) -> int> = Set.of()\n",
        "let badKeyInference = Set.of((value: int) => value)\n",
        "let badBufferKey: Set<ByteBuffer> = Set.of()\n",
        "let badSorted = [true, false].sorted()\n",
        "let badSortedBy = [1, 2].sortedBy((value: int) => [value])\n",
        "let mut mutableBools = [true, false]\n",
        "mutableBools.sort()\n",
        "let badEncoded = JSON.stringify(() => 1)\n",
        "let badDecoded = JSON.decode<float>(\"1.5\")\n",
        "let badParsed = JSON.parseAs<float>(\"1.5\")\n",
        "let missingTarget = JSON.parseAs(\"1\")\n",
        "let mapLeft: Map<string, int> = Map.new()\n",
        "let mapRight: Map<string, int> = Map.new()\n",
        "Test.assertEq(mapLeft, mapRight)\n",
        "Test.assertNe(mapLeft, mapRight)\n",
        "record BadEq derives Eq { value: Map<string, int> }\n",
        "record BadOrder derives Order { value: bool }\n",
        "record BadJson derives JSON { value: Result<int, string> }\n",
        "enum BadEnum derives Eq { Wrapped(Map<string, int>) }\n",
        "record GenericBad<T> derives Eq { value: T }\n",
        "record Duplicate derives Eq, Eq { value: int }\n",
    );
    let self_diagnostics = self_checker_diagnostics(rejected);
    assert_eq!(self_diagnostics, stage0_checker_diagnostics(rejected));
    assert!(!self_diagnostics.is_empty());

    let rejected_functions = concat!(
        "record Plain { name: string }\n",
        "record NestedBad { values: Map<string, int> }\n",
        "function encodeUnbounded<T>(value: T) -> Result<string, string> { JSON.stringify(value) }\n",
        "function useBadKey(value: NestedBad) -> () { let keys: Set<NestedBad> = Set.of(value) }\n",
        "function compareNested(left: NestedBad, right: NestedBad) -> bool { left == right }\n",
        "function requireJson<T: JSON>(value: T) -> T { value }\n",
        "let rejectedBound = requireJson(Plain { name: \"no derive\" })\n",
    );
    assert_eq!(
        self_checker_diagnostics(rejected_functions),
        stage0_checker_diagnostics(rejected_functions),
    );
}

#[test]
fn checker_loop_control_frames_match_stage0() {
    let accepted = concat!(
        "function choose(flag: bool) -> int {\n",
        "  let chosen: int = loop 'outer {\n",
        "    while false { continue }\n",
        "    for value in [1, 2] { if value > 5 { break } }\n",
        "    let projected = for value in [1] { if flag { break 'outer 1 }\nvalue }\n",
        "    break 2\n",
        "  }\n",
        "  chosen\n",
        "}\n",
        "let collected = for value in [1, 2] { value + 1 }\n",
        "let nested = [ for value in [1] => { for inner in [1] { break }\nvalue } ]\n",
    );
    assert_eq!(
        self_checker_diagnostics(accepted),
        stage0_checker_diagnostics(accepted),
    );

    let rejected = concat!(
        "break\n",
        "continue\n",
        "let unknownLabel = loop { break 'missing 1\nbreak 0 }\n",
        "let mixed = loop { if true { break 1 }\nbreak \"x\" }\n",
        "let typed: int = loop { break \"x\" }\n",
        "let blockedFor = for value in [1] { break\nvalue }\n",
        "let blockedBreak = [ for value in [1] => { break\nvalue } ]\n",
        "let blockedContinue = [ for value in [1] if { continue\ntrue } => value ]\n",
        "let lambdaBoundary = loop { let stop = () => { break }\nbreak 1 }\n",
    );
    let self_diagnostics = self_checker_diagnostics(rejected);
    assert_eq!(self_diagnostics, stage0_checker_diagnostics(rejected));
    assert!(!self_diagnostics.is_empty());
}

#[test]
fn checker_pattern_coverage_and_exhaustiveness_match_stage0() {
    let accepted = concat!(
        "type PatternMode = \"on\" | \"off\"\n",
        "enum PatternShape { Circle(int), Pair(int, int), Dot }\n",
        "record PatternUser { name: string, age: int }\n",
        "function boolMatch(value: bool) -> int { match value { case true => 1\ncase false => 0 } }\n",
        "function optionMatch(value: Option<bool>) -> int { match value { case Some(true) => 1\ncase Some(false) => 2\ncase None => 0 } }\n",
        "function resultOr(value: Result<int, int>) -> int { match value { case Ok(item) | Err(item) => item } }\n",
        "function enumMatch(value: PatternShape) -> int { match value { case Circle(radius) => radius\ncase Pair(left, right) => left + right\ncase Dot => 0 } }\n",
        "function literalMatch(value: PatternMode) -> int { match value { case \"on\" => 1\ncase \"off\" => 0 } }\n",
        "function typedMatch(value: int | string) -> int { match value { case number: int => number + 1\ncase text: string => 0 } }\n",
        "function nominalMatch(value: PatternUser | null) -> string { match value { case PatternUser { name } => name\ncase null => \"none\" } }\n",
        "function recordUnion(value: { kind: \"a\", item: int } | { kind: \"b\", item: string }) -> int { match value { case { kind: \"a\", item } => item + 1\ncase _ => 0 } }\n",
        "function listUnion(value: Array<int> | string) -> int { match value { case [item] => item\ncase _ => 0 } }\n",
    );
    assert_eq!(
        self_checker_diagnostics(accepted),
        stage0_checker_diagnostics(accepted),
    );

    let rejected = concat!(
        "enum RejectedColor { Red, Blue }\n",
        "enum RejectedShape { Circle(int), Dot }\n",
        "function impossibleTyped(value: Option<int>) -> int { match value { case item: Result<int, string> => 1\ncase _ => 0 } }\n",
        "function impossibleRecord(value: int) -> int { match value { case { item } => 1\ncase _ => 0 } }\n",
        "function impossibleList(value: int) -> int { match value { case [item] => 1\ncase _ => 0 } }\n",
        "function impossibleLiteral(value: int) -> int { match value { case \"one\" => 1\ncase _ => 0 } }\n",
        "function impossibleConstructor(value: Result<int, string>) -> int { match value { case Some(item) => 1\ncase _ => 0 } }\n",
        "function missingBool(value: bool) -> int { match value { case true => 1 } }\n",
        "function missingOption(value: Option<int>) -> int { match value { case Some(item) => item } }\n",
        "function missingResult(value: Result<int, string>) -> int { match value { case Ok(item) => item } }\n",
        "function missingEnum(value: RejectedColor) -> int { match value { case Red => 1 } }\n",
        "function typoEnum(value: RejectedColor) -> int { match value { case Reed => 1 } }\n",
        "function payloadArity(value: RejectedShape) -> int { match value { case Circle => 1\ncase Dot => 0 } }\n",
        "function emptyArity(value: RejectedShape) -> int { match value { case Circle(item) => item\ncase Dot(item) => item } }\n",
        "function orNames(value: Result<int, int>) -> int { match value { case Ok(left) | Err(right) => 0 } }\n",
        "function orTypes(value: Result<int, string>) -> int { match value { case Ok(item) | Err(item) => 0 } }\n",
        "function guardedCoverage(value: Option<int>) -> int { match value { case Some(item) if item > 0 => item\ncase None => 0 } }\n",
        "function payloadCoverage(value: Option<int>) -> int { match value { case Some(1) => 1\ncase None => 0 } }\n",
        "function builtinArity<T>(value: Option<int> | T) -> int { match value { case Some(left, right) => 1\ncase _ => 0 } }\n",
    );
    let self_diagnostics = self_checker_diagnostics(rejected);
    assert_eq!(self_diagnostics, stage0_checker_diagnostics(rejected));
    assert!(!self_diagnostics.is_empty());
}

#[test]
fn checker_nested_nominal_and_rigid_patterns_match_stage0() {
    let accepted = concat!(
        "newtype PatternId<T> = T\n",
        "enum NestedColor { Red, Blue }\n",
        "enum NestedBox { Wrap(NestedColor) }\n",
        "record PatternCell<T> { value: T }\n",
        "function unwrapId(value: PatternId<int>) -> int { match value { case PatternId(inner) => inner } }\n",
        "function nestedEnum(value: NestedBox) -> int { match value { case Wrap(Red) => 1\ncase Wrap(Blue) => 2 } }\n",
        "function genericRecord(value: PatternCell<int> | PatternCell<string>) -> string { match value { case PatternCell { value } => \"{value}\" } }\n",
        "function rigidRecord<T>(value: T) -> bool { match value { case { item } => true\ncase _ => false } }\n",
        "function rigidList<T>(value: T) -> bool { match value { case [item, ..rest] => true\ncase _ => false } }\n",
        "function rigidConstructor<T>(value: T) -> bool { match value { case Some(item) => true\ncase _ => false } }\n",
    );
    assert_eq!(
        self_checker_diagnostics(accepted),
        stage0_checker_diagnostics(accepted),
    );

    let rejected = concat!(
        "newtype RejectedId = int\n",
        "enum InnerColor { Red, Blue }\n",
        "enum OuterBox { Wrap(InnerColor) }\n",
        "enum TreeShape { Leaf(int), Branch(int, int), Empty }\n",
        "record RejectedUser { name: string, age: int }\n",
        "function useRigidRecord<T>(value: T) -> int { match value { case { item } => item\ncase _ => 0 } }\n",
        "function useRigidList<T>(value: T) -> string { match value { case [item] => item\ncase _ => \"none\" } }\n",
        "function useRigidConstructor<T>(value: T) -> int { match value { case Some(item) => item\ncase _ => 0 } }\n",
        "function missingNested(value: OuterBox) -> int { match value { case Wrap(Red) => 1 } }\n",
        "function refutableTuple(value: TreeShape) -> int { match value { case Leaf(item) => item\ncase Branch(1, right) => right\ncase Empty => 0 } }\n",
        "function refutableRecord(value: RejectedUser) -> string { match value { case RejectedUser { name: \"Ada\", age } => \"{age}\" } }\n",
        "function refutableNewtype(value: RejectedId) -> int { match value { case RejectedId(1) => 1 } }\n",
        "function missingStructuralField(value: { present: int }) -> int { match value { case { veryMissing } => 1\ncase _ => 0 } }\n",
        "function missingNominalField(value: RejectedUser) -> int { match value { case RejectedUser { veryMissing } => 1\ncase _ => 0 } }\n",
        "function typoNominalHead(value: RejectedUser) -> int { match value { case RejectdUser { name } => 1\ncase _ => 0 } }\n",
        "function duplicateNominalField(value: RejectedUser) -> string { match value { case RejectedUser { name: first, name: second } => \"{first}{second}\" } }\n",
    );
    let self_diagnostics = self_checker_diagnostics(rejected);
    assert_eq!(self_diagnostics, stage0_checker_diagnostics(rejected));
    assert!(!self_diagnostics.is_empty());
}

#[test]
fn checker_preserves_each_multi_payload_enum_pattern_type() {
    let source = concat!(
        "enum MixedPayload { Row(int, Array<int>, string) }\n",
        "function summarize(value: MixedPayload) -> int { match value {\n",
        "case Row(number, values, text) => number + values.length + text.byteLength()\n",
        "} }\n",
    );
    let self_diagnostics = self_checker_diagnostics(source);
    assert_eq!(self_diagnostics, stage0_checker_diagnostics(source));
    assert!(self_diagnostics.is_empty(), "{self_diagnostics:?}");
}

#[test]
fn checker_limits_expected_type_to_result_expressions() {
    let source = concat!(
        "function render(value: Option<int>) -> string {\n",
        "  match value {\n",
        "    case Some(item) => {\n",
        "      let nested = match Some(item) {\n",
        "        case Some(found) => found\n",
        "        case None => 0\n",
        "      }\n",
        "      \"{nested}\"\n",
        "    }\n",
        "    case None => \"\"\n",
        "  }\n",
        "}\n",
        "function absent(flag: bool) -> Option<int> {\n",
        "  if flag {\n",
        "    let nested = if flag { 1 } else { 0 }\n",
        "    None\n",
        "  } else {\n",
        "    None\n",
        "  }\n",
        "}\n",
    );
    let self_diagnostics = self_checker_diagnostics(source);
    assert_eq!(self_diagnostics, stage0_checker_diagnostics(source));
    assert!(self_diagnostics.is_empty(), "{self_diagnostics:?}");
}

#[test]
fn checker_scopes_function_result_context_to_result_expressions() {
    let source = concat!(
        "function absent() -> Option<int> {\n",
        "  let nested: Option<string> = None\n",
        "  None\n",
        "}\n",
        "function success() -> Result<int, string> {\n",
        "  let nested: Result<string, int> = Ok(\"ready\")\n",
        "  Ok(1)\n",
        "}\n",
        "function failure() -> Result<int, string> {\n",
        "  let nested: Result<string, int> = Err(1)\n",
        "  Err(\"stop\")\n",
        "}\n",
    );
    let self_diagnostics = self_checker_diagnostics(source);
    assert_eq!(self_diagnostics, stage0_checker_diagnostics(source));
    assert!(self_diagnostics.is_empty(), "{self_diagnostics:?}");
}

#[test]
fn checker_uses_protocol_static_members_and_result_context_for_generics() {
    let preview = typed_source(concat!(
        "record User derives Show { name: string }\n",
        "record Cell<T> { value: T }\n",
        "enum Box<T> { Empty, One(T) }\n",
        "function render<T: Show>(value: T) -> string { Show.show(value) }\n",
        "function empty<T>() -> Box<T> { Box.Empty }\n",
        "function wrap<T>(value: T) -> Box<T> { Box.One(value) }\n",
        "let ada = User { name: \"Ada\" }\n",
        "let grace = ada { name: \"Grace\" }\n",
        "let copied = User { ...grace, name: \"Lin\" }\n",
        "let structural = { name: \"Ada\" }\n",
        "let lastWins = structural { name: 1, name: \"Grace\" }\n",
        "let cell: Cell<int> = Cell { value: 1 }\n",
        "let nextCell = cell { value: 2 }\n",
        "let values: Array<int> = Array.of(1, 2)\n",
        "let magnitude: float = Math.abs(1.0)\n",
        "let digest: Bytes = Hash.sha256(Bytes.encodeUtf8(\"x\"))\n",
        "let projected: Result<Path, string> = Path.project(\"x\")\n",
        "let rows: Result<Array<Array<string>>, string> = CSV.parse(\"a\")\n",
        "let parsedDate: Result<Date, string> = Date.parseIso(\"2026-08-11\")\n",
        "let roundDown: RoundingMode = RoundingMode.Down\n",
        "let roundUp: RoundingMode = RoundingMode.Up\n",
        "let shown: string = render(grace)\n",
        "let absent: Box<int> = empty()\n",
        "let present: Box<int> = wrap(42)\n",
    ));
    assert!(preview.resolved.diagnostics.is_empty());
    assert!(preview.diagnostics.is_empty(), "{:?}", preview.diagnostics);
}

#[test]
fn checker_reports_contextual_control_assignment_and_return_errors() {
    let cases = [
        ("if 1 { 2 } else { 3 }\n", "expected `bool`, found `int`"),
        ("for value in 1 { value }\n", "`1` is not iterable (§10)"),
        (
            "function answer() -> int { return \"no\" }\n",
            "expected `int`, found `string`",
        ),
        (
            "function answer(value: int = \"no\") -> int { value }\n",
            "expected `int`, found `string`",
        ),
        (
            "let mut value: int = 1\nvalue = \"no\"\n",
            "expected `int`, found `string`",
        ),
        (
            "let value = 1\nvalue = 2\n",
            "`value` is immutable; declare it with `let mut`",
        ),
        (
            concat!(
                "record Pair { left: int, right: int }\n",
                "let pair = Pair { left: 1, right: 2 }\n",
                "let changed = pair { left: \"no\" }\n",
            ),
            "expected `int`, found `string`",
        ),
        (
            concat!(
                "let user = { name: \"A\" }\n",
                "let changed = user { email: \"x\" }\n",
            ),
            "`{ name: string }` has no field `email`",
        ),
        (
            "function change(value: int) -> () { let next = value { item: 1 } }\n",
            "record update needs a record, found `int`",
        ),
        (
            "record Pair { left: int, right: int }\nlet pair = Pair { left: 1 }\n",
            "record `Pair` is missing field `right`",
        ),
        (
            concat!(
                "record Pair { left: int, right: int }\n",
                "let pair = Pair { left: 1, left: 2, right: 3 }\n",
            ),
            "field `left` is given twice in `Pair`",
        ),
        (
            concat!(
                "record Pair { left: int, right: int }\n",
                "let pair = Pair { left: 1, right: 2, extra: 3 }\n",
            ),
            "record `Pair` has no field `extra`",
        ),
        (
            "let value = Math.missing(1.0)\n",
            "`Math` has no member named `missing`",
        ),
    ];
    for (source, message) in cases {
        let preview = typed_source(source);
        assert!(preview.resolved.diagnostics.is_empty(), "{source}");
        assert!(
            preview
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == message),
            "{source}: {:?}",
            preview.diagnostics,
        );
    }
}

#[test]
fn checker_statement_binding_return_and_branch_agreement_matches_stage0() {
    let record_types = |source: &str, kind: topaz_hir::TypedNodeKind| {
        let parsed = topaz_parser::parse_with_options(
            FileId(0),
            source,
            topaz_parser::ParseOptions {
                language_version: LangVersion::CURRENT,
            },
        );
        let stage0 = topaz_check::check_program_typed_with_version(
            source,
            &parsed.program,
            LangVersion::CURRENT,
        )
        .typed_hir
        .unwrap()
        .nodes
        .into_iter()
        .filter(|node| node.kind == kind && matches!(node.ty, topaz_hir::SemanticType::Record(_)))
        .map(|node| node.ty)
        .collect::<Vec<_>>();
        let self_hosted = typed_source(source)
            .nodes
            .into_iter()
            .filter(|node| {
                node.kind == kind && matches!(node.ty, topaz_hir::SemanticType::Record(_))
            })
            .map(|node| node.ty)
            .collect::<Vec<_>>();
        (self_hosted, stage0)
    };
    let record_source = "const reverseRecord = { z: 1, a: \"x\" }\n";
    let (self_record_bindings, stage0_record_bindings) =
        record_types(record_source, topaz_hir::TypedNodeKind::Binding);
    let (self_contextual_records, stage0_contextual_records) = record_types(
        "let tagged: { kind: \"open\" } = { kind: \"open\" }\n",
        topaz_hir::TypedNodeKind::Expression,
    );
    let expected_record_bindings = vec![topaz_hir::SemanticType::Record(vec![
        topaz_hir::SemanticField {
            name: "a".to_string(),
            ty: topaz_hir::SemanticType::Primitive(topaz_hir::SemanticPrimitive::String),
        },
        topaz_hir::SemanticField {
            name: "z".to_string(),
            ty: topaz_hir::SemanticType::Primitive(topaz_hir::SemanticPrimitive::Int),
        },
    ])];
    let expected_contextual_records = vec![topaz_hir::SemanticType::Record(vec![
        topaz_hir::SemanticField {
            name: "kind".to_string(),
            ty: topaz_hir::SemanticType::Literal(topaz_hir::SemanticLiteral::String(
                "open".to_string(),
            )),
        },
    ])];
    let duplicate_records = concat!(
        "type R = { x: int, x: string }\n",
        "let r = { x: 1, x: 2 }\n",
    );
    let expected_duplicate_record_diagnostics = vec![
        (
            "TPZ5022".to_string(),
            "record type declares field `x` twice".to_string(),
            19,
            28,
        ),
        (
            "TPZ5022".to_string(),
            "record literal declares field `x` twice".to_string(),
            47,
            51,
        ),
    ];
    let accepted = concat!(
        "function unitReturn() -> () { return }\n",
        "function inferredReturn(flag: bool) { if flag { return 1 }\nreturn 2 }\n",
        "let inferredValue: int = inferredReturn(true)\n",
        "function inferredBranches(flag: bool) { if flag { return 1 } else { return 2 } }\n",
        "let inferredBranchValue: int = inferredBranches(true)\n",
        "function explicitBranches(flag: bool) -> int { if flag { return 1 } else { return 2 } }\n",
        "let lambdaReturn = (value: int) => { if value > 0 { return 1 }\nreturn 2 }\n",
        "let lambdaValue: int = lambdaReturn(1)\n",
        "let mut optional: Option<int> = None\n",
        "optional ??= Some(1)\n",
        "let mut nullable: string | null = null\n",
        "nullable ??= \"ready\"\n",
        "let mut number = 1\n",
        "number += 2\n",
        "number -= 1\n",
        "number *= 3\n",
        "number /= 2\n",
        "number %= 2\n",
        "let mut appendable = \"a\"\n",
        "appendable += \"b\"\n",
        "let mut nestedRecord = { inner: { count: 1 } }\n",
        "nestedRecord.inner.count += 1\n",
        "let mut nestedCells = [{ count: 1 }]\n",
        "nestedCells[0].count = 2\n",
        "let branch: int = if true { 1 } else { 2 }\n",
        "let tagged: { kind: \"open\" } = { kind: \"open\" }\n",
        "let ordered: { a: string, z: int } = { z: 1, a: \"x\" }\n",
        "function inferredResult(flag: bool) { if flag { return Ok(1) }\nreturn Err(\"e\") }\n",
        "let joinedResult: Result<int, string> = inferredResult(true)\n",
        "let lambdaResult = (flag: bool) => if flag { return Ok(1) } else { return Err(\"e\") }\n",
        "let joinedLambdaResult: Result<int, string> = lambdaResult(true)\n",
        "function inferredTailResult(flag: bool) { if flag { Ok(1) } else { Err(\"e\") } }\n",
        "let joinedTailResult: Result<int, string> = inferredTailResult(true)\n",
        "function inferredOption(flag: bool) { if flag { return Some(1) }\nreturn None }\n",
        "let joinedOption: Option<int> = inferredOption(true)\n",
        "record OptionalPayload { value: int }\n",
        "function optionalPayloadValue(value: Option<OptionalPayload>) -> int {\n",
        "  let payload = match value {\n",
        "    case Some(found) => found\n",
        "    case None => { return 0 }\n",
        "  }\n",
        "  payload.value\n",
        "}\n",
        "function inferredArray(flag: bool) { if flag { return [1] }\nreturn [] }\n",
        "let joinedArray: Array<int> = inferredArray(true)\n",
        "function completedAliasSource() { return 0 }\n",
        "let completedAlias = completedAliasSource\n",
        "function completedAliasCaller() { return completedAlias() }\n",
        "let completedAliasValue: int = completedAliasCaller()\n",
    );
    assert_eq!(
        (
            self_checker_diagnostics(accepted),
            self_record_bindings,
            stage0_record_bindings,
            self_contextual_records,
            stage0_contextual_records,
            self_checker_diagnostics(duplicate_records),
            stage0_checker_diagnostics(duplicate_records),
        ),
        (
            stage0_checker_diagnostics(accepted),
            expected_record_bindings.clone(),
            expected_record_bindings,
            expected_contextual_records.clone(),
            expected_contextual_records,
            expected_duplicate_record_diagnostics.clone(),
            expected_duplicate_record_diagnostics,
        ),
    );

    let rejected = concat!(
        "return 1\n",
        "function bareReturn() -> int { return }\n",
        "function badUsing(value: int) -> () { using handle = value { () } }\n",
        "let mut scalar = 1\n",
        "scalar ??= 2\n",
        "let mut optionalText: Option<string> = None\n",
        "optionalText ??= Some(1)\n",
        "let immutableOptional: Option<int> = None\n",
        "immutableOptional ??= Some(1)\n",
        "let mut wrappedOptional: Option<int> = None\n",
        "wrappedOptional ??= 1\n",
        "let mut values: Map<string, int> = Map.new()\n",
        "values[\"key\"] = 1\n",
        "let mut mappedRecords: Map<string, { count: int }> = Map.new()\n",
        "mappedRecords[\"key\"].count = 1\n",
        "let mut text = \"a\"\n",
        "text -= 1\n",
        "let immutableRecord = { inner: { count: 1 } }\n",
        "immutableRecord.inner.count += 1\n",
        "let mut wrongCells = [1]\n",
        "wrongCells[0] = \"one\"\n",
        "function inferredMismatch(flag: bool) { if flag { return 1 }\nreturn 2 }\n",
        "let badInferred: string = inferredMismatch(true)\n",
        "let badBranch: int = if true { \"no\" } else { 2 }\n",
        "let badMatchReturn = match true { case true => return 1\ncase false => 0 }\n",
        "function badBareMatchReturn() -> int { match true { case true => return\ncase false => 0 } }\n",
        "function recursiveMissingReturn(value: int) { if value == 0 { return 0 }\nreturn recursiveMissingReturn(value - 1) }\n",
        "function mutuallyA(value: int) { if value == 0 { return 0 }\nreturn mutuallyB(value - 1) }\n",
        "function mutuallyB(value: int) { if value == 0 { return 0 }\nreturn mutuallyA(value - 1) }\n",
        "function aliasedRecursive(value: int) { let again = aliasedRecursive\nif value == 0 { return 0 }\nreturn again(value - 1) }\n",
        "function pendingAliasCaller() { let pendingAlias = pendingAliasSource\nreturn pendingAlias() }\n",
        "function pendingAliasSource() { return pendingAliasCaller() }\n",
        "function projectedReturn<T>(value: T) { return value.item }\n",
        "function projectedPatternReturn<T>(value: T) { return match value { case { item } => item\ncase _ => value } }\n",
        "function concreteProjectedReturn<T>(value: T) -> int { return value.item }\n",
    );
    let self_diagnostics = self_checker_diagnostics(rejected);
    assert_eq!(self_diagnostics, stage0_checker_diagnostics(rejected));
    assert!(!self_diagnostics.is_empty());
}

#[test]
fn checker_contextual_lambda_arity_matches_stage0() {
    let source = "let result: Result<int, int> = (None).okOrElse((value: int) => value)\n";
    let expected = vec![
        (
            "TPZ5004".to_string(),
            "this lambda takes 1 parameter(s), but a function taking 0 is expected".to_string(),
            47,
            68,
        ),
        (
            "TPZ5001".to_string(),
            "expected `() -> int`, found `(int) -> int`".to_string(),
            47,
            68,
        ),
    ];
    assert_eq!(stage0_checker_diagnostics(source), expected);
    assert_eq!(self_checker_diagnostics(source), expected);
}

#[test]
fn checker_non_simple_duplicate_map_keys_remain_runtime_owned() {
    for source in [
        "let values = map { 1.5: 1, 1.5: 2 }\n",
        "let values = map { (): 1, (): 2 }\n",
    ] {
        let expected = Vec::new();
        assert_eq!(stage0_checker_diagnostics(source), expected);
        assert_eq!(self_checker_diagnostics(source), expected);
    }
}

#[test]
fn checker_const_default_initialization_and_callable_alias_agreement_matches_stage0() {
    const INTERPOLATED_DEFAULT: &str =
        "function invalid(value: string = \"value {1}\") -> string { value }\n";
    let accepted = [
        concat!(
            "let value = LATER\n",
            "const LATER = 7\n",
            "function readsLater() -> int { runtimeValue }\n",
            "let runtimeValue = 9\n",
            "let observed: int = readsLater()\n",
        ),
        concat!(
            "function greet(name: string, suffix: string = \"!\") -> string { \"{name}{suffix}\" }\n",
            "let wrapped = ({ greet })\n",
            "let message: string = wrapped(name: \"Topaz\")\n",
            "function identity<T>(value: T) -> T { value }\n",
            "const genericAlias = identity\n",
            "let explicit: string = genericAlias<string>(\"ready\")\n",
        ),
        concat!(
            "function configured(count: int = 1 + 2, label: string = \"ready\") -> int { count }\n",
            "let count: int = configured()\n",
        ),
        concat!(
            "function first() -> int { second() }\n",
            "let captured = first\n",
            "let throughBody = captured()\n",
            "let throughLambda = (() => later())()\n",
            "let throughBranch = if false { later() } else { 0 }\n",
            "function second() -> int { 2 }\n",
            "function later() -> int { 3 }\n",
            "const SHORT_CIRCUIT = (1 / 0 == 0) && true\n",
        ),
        concat!(
            "let shortAnd: bool = false && laterBool\n",
            "let shortOr: bool = true || laterBool\n",
            "let present: Option<int> = Some(1)\n",
            "let coalesced: int = present ?? laterInt\n",
            "let receiver: Option<string> = None\n",
            "let optionalCall: string = receiver?.replace(laterText, \"y\") ?? \"skipped\"\n",
            "let laterBool: bool = true\n",
            "let laterInt: int = 2\n",
            "let laterText: string = \"x\"\n",
        ),
    ];
    let mut differences = Vec::new();
    for source in accepted {
        let self_diagnostics = self_checker_diagnostics(source);
        let stage0_diagnostics = stage0_checker_diagnostics(source);
        if self_diagnostics != stage0_diagnostics || !stage0_diagnostics.is_empty() {
            differences.push(format!(
                "{source}\nself={self_diagnostics:?}\nstage0={stage0_diagnostics:?}"
            ));
        }
    }

    let rejected = [
        "let value = later()\nfunction later() -> int { 1 }\n",
        "let first = second\nlet second = 2\n",
        "const SECOND = FIRST + 1\nconst FIRST = 1\n",
        "function invalid(value: string = input()) -> string { value }\n",
        "function invalid(value: int = 1 / 0) -> int { value }\n",
        concat!(
            "const DIVIDE = 1 / 0\n",
            "const REMAINDER = 5 % 0\n",
            "const ADD = 9223372036854775807 + 1\n",
            "const SUBTRACT = -9223372036854775807 - 2\n",
            "const MULTIPLY = 9223372036854775807 * 2\n",
            "const DIVIDE_OVERFLOW = (-9223372036854775807 - 1) / -1\n",
            "const REMAINDER_OVERFLOW = (-9223372036854775807 - 1) % -1\n",
            "const NEGATE_OVERFLOW = -(-9223372036854775807 - 1)\n",
            "const NEGATIVE_POWER = 2 ** -1\n",
            "const POWER = 10 ** 100\n",
        ),
        concat!(
            "let arrayValue = [arrayLater()]\n",
            "function arrayLater() -> int { 1 }\n",
            "let recordValue = { item: recordLater() }\n",
            "function recordLater() -> int { 2 }\n",
            "let pipeValue = 0 |> pipeLater\n",
            "function pipeLater(value: int) -> int { value }\n",
        ),
        concat!(
            "function greet(name: string, suffix: string = \"!\") -> string { \"{name}{suffix}\" }\n",
            "let wrapped = ({ greet })\n",
            "let message = wrapped()\n",
        ),
        concat!(
            "function identity<T>(value: T) -> T { value }\n",
            "const genericAlias = identity\n",
            "let explicit = genericAlias<int>()\n",
        ),
    ];
    for source in rejected {
        let self_diagnostics = self_checker_diagnostics(source);
        let stage0_diagnostics = stage0_checker_diagnostics(source);
        if self_diagnostics != stage0_diagnostics {
            differences.push(format!(
                "{source}\nself={self_diagnostics:?}\nstage0={stage0_diagnostics:?}"
            ));
        }
    }
    let expected_interpolated_default = vec![(
        "TPZ5001".to_string(),
        "`const` initializers must be constant expressions (§4)".to_string(),
        33,
        44,
    )];
    assert_eq!(
        (
            differences,
            stage0_checker_diagnostics(INTERPOLATED_DEFAULT),
            self_checker_diagnostics(INTERPOLATED_DEFAULT),
        ),
        (
            Vec::<String>::new(),
            expected_interpolated_default.clone(),
            expected_interpolated_default,
        ),
    );
}

#[test]
fn checker_capture_scope_import_and_alias_agreement_matches_stage0() {
    let nested = concat!(
        "record Marker { value: int }\n",
        "let global = 1\n",
        "function identity<T>(value: T) -> T { value }\n",
        "let callableAlias = identity\n",
        "function make(seed: int) {\n",
        "  let localAlias = callableAlias\n",
        "  () => {\n",
        "    let middle = seed\n",
        "    () => localAlias<int>(global + middle)\n",
        "  }\n",
        "}\n",
        "let typedOnly = () => {\n",
        "  let value: Marker = Marker { value: 1 }\n",
        "  value.value\n",
        "}\n",
    );
    let self_nested = typed_source(nested);
    assert!(
        self_nested.diagnostics.is_empty(),
        "{:?}",
        self_nested.diagnostics
    );
    let nested_request = topaz_kernel::KernelRequest::checked(
        "main.tpz",
        Some(""),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    let stage0_nested = stage0_typed_captures(&SourceFixtureHost(nested), nested_request);
    assert_eq!(
        canonical_capture_files(self_nested.captures.clone()),
        canonical_capture_files(stage0_nested),
    );
    let nested_counts = self_nested.captures.iter().fold(
        std::collections::BTreeMap::<String, usize>::new(),
        |mut counts, capture| {
            *counts.entry(capture.name.clone()).or_default() += 1;
            counts
        },
    );
    assert_eq!(
        nested_counts,
        std::collections::BTreeMap::from([
            ("callableAlias".to_string(), 1),
            ("global".to_string(), 3),
            ("localAlias".to_string(), 2),
            ("middle".to_string(), 1),
            ("seed".to_string(), 1),
        ])
    );
    assert!(
        self_nested
            .captures
            .iter()
            .all(|capture| capture.name != "Marker")
    );

    let host = CaptureImportFixtureHost;
    let import_request = resolver_request().with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    let self_imports =
        preview_typed(&host, import_request.clone()).expect("self-hosted imported capture fixture");
    assert!(
        self_imports.resolved.diagnostics.is_empty(),
        "{:?}",
        self_imports.resolved.diagnostics
    );
    assert!(
        self_imports.diagnostics.is_empty(),
        "{:?}",
        self_imports.diagnostics
    );
    let stage0_imports = stage0_typed_captures(&host, import_request);
    assert_eq!(
        canonical_capture_files(self_imports.captures.clone()),
        canonical_capture_files(stage0_imports),
    );
    assert_eq!(
        self_imports
            .captures
            .iter()
            .map(|capture| capture.name.as_str())
            .collect::<Vec<_>>(),
        ["value", "ns", "alias"]
    );
    assert_eq!(
        self_imports
            .captures
            .iter()
            .filter(|capture| capture.ambient)
            .map(|capture| capture.name.as_str())
            .collect::<Vec<_>>(),
        ["ns"]
    );
    assert!(
        self_imports
            .captures
            .iter()
            .all(|capture| capture.name != "Marker")
    );
}

#[test]
fn checker_named_type_and_receiver_catalog_matches_stage0() {
    let preview = typed_source(concat!(
        "function mapAlias(values: Map<string, int>) -> () {\n",
        "  let present = values.contains(\"key\")\n",
        "}\n",
        "function bytesAlias(bytes: Bytes) -> () {\n",
        "  let encoded = bytes.encodeUtf8(\"x\")\n",
        "}\n",
        "function unrelatedName(text: string) -> () {\n",
        "  let removed = text.remove()\n",
        "}\n",
        "function jsonAlias(value: JSONValue) -> () {\n",
        "  let parsed = value.parseAs(\"[]\")\n",
        "  let encoded = value.stringify(value)\n",
        "}\n",
    ));
    let messages = preview
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        messages,
        std::collections::BTreeSet::from([
            "`Bytes` has no member named `encodeUtf8`",
            "`JSONValue` has no member named `stringify`",
            "`JSONValue` has no member named `parseAs`",
            "`Map<string, int>` has no member named `contains`",
            "`string` has no member named `remove`",
        ]),
    );
    assert!(
        preview
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "TPZ5003"),
        "{:?}",
        preview.diagnostics,
    );

    let mutator = typed_source("let values = [1]\nlet push = values.push\n");
    assert_eq!(mutator.diagnostics.len(), 1, "{:?}", mutator.diagnostics);
    assert_eq!(mutator.diagnostics[0].code, "TPZ5003");

    fn fixture_type(ty: &topaz_check::Type) -> String {
        use topaz_check::{Ctor, Lit, Type};

        match ty {
            Type::Prim(primitive) => primitive.name().to_string(),
            Type::Literal(Lit::Str(_)) => "string".to_string(),
            Type::Literal(Lit::Int(_)) => "int".to_string(),
            Type::Literal(Lit::Float(_)) => "float".to_string(),
            Type::Literal(Lit::Bool(_)) => "bool".to_string(),
            Type::Literal(Lit::Null) => "null".to_string(),
            Type::Union(members) => members
                .iter()
                .map(fixture_type)
                .collect::<Vec<_>>()
                .join(" | "),
            Type::Record(fields) => format!(
                "{{ {} }}",
                fields
                    .iter()
                    .map(|(name, ty)| format!("{name}: {}", fixture_type(ty)))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Type::Ctor(constructor, arguments) => {
                assert_ne!(*constructor, Ctor::Range);
                format!(
                    "{}<{}>",
                    constructor.name(),
                    arguments
                        .iter()
                        .map(fixture_type)
                        .collect::<Vec<_>>()
                        .join(", "),
                )
            }
            Type::Func {
                params,
                variadic,
                ret,
            } => {
                assert!(variadic.is_none());
                format!(
                    "({}) -> {}",
                    params
                        .iter()
                        .map(fixture_type)
                        .collect::<Vec<_>>()
                        .join(", "),
                    fixture_type(ret),
                )
            }
            Type::Foreign { name, args }
            | Type::Enum { base: name, args }
            | Type::NominalRecord { base: name, args }
            | Type::Newtype { base: name, args } => {
                if args.is_empty() {
                    name.clone()
                } else {
                    format!(
                        "{name}<{}>",
                        args.iter().map(fixture_type).collect::<Vec<_>>().join(", "),
                    )
                }
            }
            Type::Skolem { name, .. } => name.clone(),
            Type::Template => "template".to_string(),
            Type::File => "File".to_string(),
            Type::JsonValue => "JSONValue".to_string(),
            Type::Bytes => "Bytes".to_string(),
            Type::ByteBuffer => "ByteBuffer".to_string(),
            Type::Path => "Path".to_string(),
            Type::Regex => "Regex".to_string(),
            Type::Match => "Match".to_string(),
            Type::TomlValue => "TOMLValue".to_string(),
            Type::Url => "URL".to_string(),
            Type::Date => "Date".to_string(),
            Type::BigInt => "BigInt".to_string(),
            Type::Decimal => "Decimal".to_string(),
            Type::RoundingMode => "RoundingMode".to_string(),
            Type::Var(_) | Type::Unknown => "string".to_string(),
        }
    }

    fn fixture_value(ty: &topaz_check::Type, receiver: &topaz_check::Type) -> String {
        use topaz_check::{Ctor, Lit, Prim, Type};

        if ty == receiver {
            return "subject".to_string();
        }
        match ty {
            Type::Prim(Prim::Int) | Type::Literal(Lit::Int(_)) => "1".to_string(),
            Type::Prim(Prim::Float) | Type::Literal(Lit::Float(_)) => "1.0".to_string(),
            Type::Prim(Prim::String) | Type::Literal(Lit::Str(_)) => "\"value\"".to_string(),
            Type::Prim(Prim::Bool) | Type::Literal(Lit::Bool(_)) => "true".to_string(),
            Type::Prim(Prim::Unit) => "()".to_string(),
            Type::Literal(Lit::Null) => "null".to_string(),
            Type::Ctor(Ctor::Option, arguments) => {
                format!("Some({})", fixture_value(&arguments[0], receiver))
            }
            Type::Ctor(Ctor::Result, arguments) => {
                format!("Ok({})", fixture_value(&arguments[0], receiver))
            }
            Type::Ctor(Ctor::Array, arguments) => {
                format!("[{}]", fixture_value(&arguments[0], receiver))
            }
            Type::Ctor(Ctor::Set, arguments) => {
                format!("Set.of({})", fixture_value(&arguments[0], receiver))
            }
            Type::Ctor(Ctor::Map, arguments) => format!(
                "Map.ofEntries([{{ key: {}, value: {} }}])",
                fixture_value(&arguments[0], receiver),
                fixture_value(&arguments[1], receiver),
            ),
            Type::Func { params, ret, .. } => {
                let names = (0..params.len())
                    .map(|index| format!("value{index}"))
                    .collect::<Vec<_>>();
                let head = if names.len() == 1 {
                    names[0].clone()
                } else {
                    format!("({})", names.join(", "))
                };
                format!("{head} => {}", fixture_value(ret, receiver))
            }
            Type::RoundingMode => "RoundingMode.Down".to_string(),
            Type::Var(_) | Type::Unknown => "\"value\"".to_string(),
            other => panic!("receiver catalog has no fixture value for {other:?}"),
        }
    }

    let int = topaz_check::Type::Prim(topaz_check::Prim::Int);
    let string = topaz_check::Type::Prim(topaz_check::Prim::String);
    let receivers = [
        (int.clone(), "int"),
        (string.clone(), "string"),
        (
            topaz_check::Type::Ctor(topaz_check::Ctor::Option, vec![int.clone()]),
            "Option<int>",
        ),
        (
            topaz_check::Type::Ctor(topaz_check::Ctor::Result, vec![int.clone(), string.clone()]),
            "Result<int, string>",
        ),
        (
            topaz_check::Type::Ctor(topaz_check::Ctor::Array, vec![int.clone()]),
            "Array<int>",
        ),
        (
            topaz_check::Type::Ctor(topaz_check::Ctor::Map, vec![string.clone(), int.clone()]),
            "Map<string, int>",
        ),
        (
            topaz_check::Type::Ctor(topaz_check::Ctor::Set, vec![int]),
            "Set<int>",
        ),
        (topaz_check::Type::JsonValue, "JSONValue"),
        (topaz_check::Type::Bytes, "Bytes"),
        (topaz_check::Type::ByteBuffer, "ByteBuffer"),
        (topaz_check::Type::Path, "Path"),
        (topaz_check::Type::Regex, "Regex"),
        (topaz_check::Type::Match, "Match"),
        (topaz_check::Type::Url, "URL"),
        (topaz_check::Type::Date, "Date"),
        (topaz_check::Type::BigInt, "BigInt"),
        (topaz_check::Type::Decimal, "Decimal"),
    ];
    let mut source = String::new();
    let mut observation = 0;
    for (receiver_index, (receiver, type_text)) in receivers.iter().enumerate() {
        source.push_str(&format!(
                "function exerciseReceiver{receiver_index}(input: {type_text}) -> () {{\n  let mut subject = input\n"
            ));
        for member_name in topaz_check::builtins::receiver_member_names(receiver) {
            let member = topaz_check::builtins::receiver_member(receiver, member_name)
                .expect("member name and member table agree");
            match member {
                topaz_check::builtins::Member::Property(property) => {
                    source.push_str(&format!(
                        "  let observed{observation}: {} = subject.{member_name}\n",
                        fixture_type(&property),
                    ));
                }
                topaz_check::builtins::Member::Method(scheme) => {
                    let arguments = scheme
                        .params
                        .iter()
                        .zip(&scheme.names)
                        .rev()
                        .map(|(parameter, name)| {
                            format!("{name}: {}", fixture_value(parameter, receiver))
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    source.push_str(&format!(
                        "  let observed{observation}: {} = subject.{member_name}({arguments})\n",
                        fixture_type(&scheme.ret),
                    ));
                }
            }
            observation += 1;
        }
        if *receiver == topaz_check::Type::Decimal {
            source.push_str(
                    "  let defaultRound: Decimal = subject.round(scale: 1)\n  let defaultDiv: Result<Decimal, string> = subject.div(scale: 1, other: subject)\n",
                );
        }
        source.push_str("}\n");
    }
    source.push_str(concat!(
        "function templateFactory() -> template { sql\"a {1} b\" }\n",
        "let templateSubject: template = templateFactory()\n",
        "let templateTag: string = templateSubject.tag\n",
        "let templateParts: Array<string> = templateSubject.parts\n",
        "let fileResult: Result<File, string> = open(\"fixture.txt\")\n",
        "match open(\"fixture.txt\") {\n",
        "  case Ok(fileSubject) => {\n",
        "    let fileRead: Result<string, string> = fileSubject.read()\n",
        "    let fileWrite: Result<(), string> = fileSubject.write(s: \"value\")\n",
        "    let fileClose: () = fileSubject.close()\n",
        "  }\n",
        "  case Err(_) => ()\n",
        "}\n",
    ));
    let source = source.as_str();
    let malformed_named_types = concat!(
        "type Scalar = int\n",
        "record Plain { value: int }\n",
        "record Box<T> { value: T }\n",
        "enum Choice<T> { Item(T) }\n",
        "newtype Wrap<T> = T\n",
        "function generic<T>(value: T<int>) -> () {}\n",
        "function primitive(value: int<string>) -> () {}\n",
        "function arrayMissing(value: Array) -> () {}\n",
        "function mapMissing(value: Map<int>) -> () {}\n",
        "function setExtra(value: Set<int, string>) -> () {}\n",
        "function optionMissing(value: Option) -> () {}\n",
        "function resultMissing(value: Result<int>) -> () {}\n",
        "function templateExtra(value: template<string>) -> () {}\n",
        "function fileExtra(value: File<int>) -> () {}\n",
        "function bytesExtra(value: Bytes<int>) -> () {}\n",
        "function plainExtra(value: Plain<int>) -> () {}\n",
        "function boxMissing(value: Box) -> () {}\n",
        "function choiceMissing(value: Choice) -> () {}\n",
        "function wrapMissing(value: Wrap) -> () {}\n",
        "function aliasExtra(value: Scalar<int>) -> () {}\n",
        "let fileResult: Result<File, string> = open(\"missing\")\n",
        "let namedUnit: unit = ()\n",
        "protocol Probe { function accepted(value: Self) -> int }\n",
        "record ProbeRecord {}\n",
        "impl Probe<ProbeRecord> { function unknown(value: Array) -> int { 1 } }\n",
        "function aliasNested(value: Scalar<Array>) -> () {}\n",
    );
    let expected_malformed_named_types = vec![
        (
            "TPZ5022".to_string(),
            "`Array` takes 1 type argument, found 0".to_string(),
            1042,
            1047,
        ),
        (
            "TPZ5022".to_string(),
            "protocol `Probe` has no method `unknown`".to_string(),
            1027,
            1034,
        ),
        (
            "TPZ5022".to_string(),
            "`impl Probe<ProbeRecord>` is missing method `accepted`".to_string(),
            997,
            1002,
        ),
        (
            "TPZ5022".to_string(),
            "type parameter `T` takes no type arguments".to_string(),
            147,
            148,
        ),
        (
            "TPZ5022".to_string(),
            "primitive type `int` takes no type arguments".to_string(),
            190,
            193,
        ),
        (
            "TPZ5022".to_string(),
            "`Array` takes 1 type argument, found 0".to_string(),
            241,
            246,
        ),
        (
            "TPZ5022".to_string(),
            "`Map` takes 2 type arguments, found 1".to_string(),
            284,
            287,
        ),
        (
            "TPZ5022".to_string(),
            "`Set` takes 1 type argument, found 2".to_string(),
            328,
            331,
        ),
        (
            "TPZ5022".to_string(),
            "`Option` takes 1 type argument, found 0".to_string(),
            385,
            391,
        ),
        (
            "TPZ5022".to_string(),
            "`Result` takes 2 type arguments, found 1".to_string(),
            432,
            438,
        ),
        (
            "TPZ5022".to_string(),
            "`template` takes no type arguments".to_string(),
            484,
            492,
        ),
        (
            "TPZ5022".to_string(),
            "`File` takes no type arguments".to_string(),
            537,
            541,
        ),
        (
            "TPZ5022".to_string(),
            "`Bytes` takes no type arguments".to_string(),
            584,
            589,
        ),
        (
            "TPZ5022".to_string(),
            "record `Plain` takes 0 type arguments, found 1".to_string(),
            632,
            637,
        ),
        (
            "TPZ5022".to_string(),
            "record `Box` takes 1 type argument, found 0".to_string(),
            680,
            683,
        ),
        (
            "TPZ5022".to_string(),
            "enum `Choice` takes 1 type argument, found 0".to_string(),
            724,
            730,
        ),
        (
            "TPZ5022".to_string(),
            "newtype `Wrap` takes 1 type argument, found 0".to_string(),
            769,
            773,
        ),
        (
            "TPZ5022".to_string(),
            "type alias `Scalar` takes 0 type arguments, found 1".to_string(),
            811,
            817,
        ),
        (
            "TPZ5022".to_string(),
            "type alias `Scalar` takes 0 type arguments, found 1".to_string(),
            1092,
            1098,
        ),
        (
            "TPZ5001".to_string(),
            "expected `unit`, found `()`".to_string(),
            910,
            912,
        ),
    ];
    let rejected = concat!("record template {\n", "  value: int,\n", "}\n",);
    let expected_rejected = vec![(
        "TPZ5022".to_string(),
        "record name `template` collides with the builtin `template` type; choose another name"
            .to_string(),
        7,
        15,
    )];
    let rejected_duplicate_method = concat!(
        "record R {}\n",
        "impl R { function f(self) -> int { 1 } function f(self, value: Array) -> int { 1 } }\n",
        "0\n",
    );
    let expected_rejected_duplicate_method = vec![(
        "TPZ5008".to_string(),
        "method `f` is already defined for `R`".to_string(),
        60,
        61,
    )];
    assert_eq!(
        (
            stage0_checker_diagnostics(source),
            self_checker_diagnostics(source),
            stage0_checker_diagnostics(malformed_named_types),
            self_checker_diagnostics(malformed_named_types),
            stage0_checker_diagnostics(rejected),
            self_checker_diagnostics(rejected),
            stage0_checker_diagnostics(rejected_duplicate_method),
            self_checker_diagnostics(rejected_duplicate_method),
        ),
        (
            Vec::<(String, String, u32, u32)>::new(),
            Vec::<(String, String, u32, u32)>::new(),
            expected_malformed_named_types.clone(),
            expected_malformed_named_types,
            expected_rejected.clone(),
            expected_rejected,
            expected_rejected_duplicate_method.clone(),
            expected_rejected_duplicate_method,
        ),
    );
}

#[test]
fn checker_result_flat_map_callback_uses_context_like_stage0() {
    let source = concat!(
        "let subject: Result<int, string> = Ok(1)\n",
        "let observed: Result<string, string> = subject.flatMap(f: value => Ok(\"value\"))\n",
    );
    let self_diagnostics = self_checker_diagnostics(source);
    assert_eq!(self_diagnostics, stage0_checker_diagnostics(source));
    assert!(self_diagnostics.is_empty(), "{self_diagnostics:?}");
}

#[test]
fn checker_concurrent_timeout_millisecond_bounds_match_stage0() {
    let accepted = concat!(
        "let by_ms = concurrent(timeout: 18446744073709551615ms) { a: 1 } else { { a: 0 } }\n",
        "let by_s = concurrent(timeout: 18446744073709551s) { a: 1 } else { { a: 0 } }\n",
        "let by_m = concurrent(timeout: 307445734561825m) { a: 1 } else { { a: 0 } }\n",
    );
    let rejected = concat!(
        "let by_ms = concurrent(timeout: 18446744073709551616ms) { a: 1 } else { { a: 0 } }\n",
        "let by_s = concurrent(timeout: 18446744073709552s) { a: 1 } else { { a: 0 } }\n",
        "let by_m = concurrent(timeout: 307445734561826m) { a: 1 } else { { a: 0 } }\n",
    );
    let accepted_self = self_checker_diagnostics(accepted);
    assert_eq!(accepted_self, stage0_checker_diagnostics(accepted));
    assert!(accepted_self.is_empty(), "{accepted_self:?}");
    assert_eq!(
        self_checker_diagnostics(rejected),
        stage0_checker_diagnostics(rejected)
    );
}
