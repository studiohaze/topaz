//! Explicit multi-module and extern fixture catalog.

use topaz_syntax::LangVersion;

use crate::build_support::model::{
    ExternModuleFixtureDef, ModuleFixtureDef, VersionedModuleFixtureDef,
};

/// The E-3 ELIGIBLE multi-module set. Every source remains inline in this small index.
pub(crate) const MODULE_FIXTURES: &[ModuleFixtureDef] = &[
    (
        "mod_local_protocols_disjoint",
        "main.tpz",
        &[
            (
                "main.tpz",
                "import left\nimport right\n\"{left.run()}/{right.run()}\"",
            ),
            (
                "left.tpz",
                "export record Item { name: string }\nimpl Show<Item> { function show(value: Item) -> string { \"L:{value.name}\" } }\nexport function run() -> string { Show.show(Item { name: \"x\" }) }",
            ),
            (
                "right.tpz",
                "export record Item { name: string }\nimpl Show<Item> { function show(value: Item) -> string { \"R:{value.name}\" } }\nexport function run() -> string { Show.show(Item { name: \"y\" }) }",
            ),
        ],
    ),
    (
        "mod_imported_derived_show",
        "main.tpz",
        &[
            ("main.tpz", "import model\nShow.show(model.make())"),
            (
                "model.tpz",
                "export record User derives Show { name: string }\nexport function make() -> User { User { name: \"Ada\" } }",
            ),
        ],
    ),
    // v5.4 `std` root: these modules are virtual resolver inputs, not physical
    // files in the fixture map. This pins that `std.math`/`std.bytes`/`std.hash`
    // still lower as ordinary module records and run≡build through the shared
    // stdlib leaves.
    (
        "mod_std_virtual_core",
        "main.tpz",
        &[(
            "main.tpz",
            "import std.math\nimport std.bytes\nimport std.hash\nimport std.path\nlet b = bytes.encodeUtf8(\"abc\")\nlet digest = hash.sha256(b).toHex()\nlet p = match path.from(\"src//./main.tpz\") {\n  case Ok(pathValue) => pathValue.toString()\n  case Err(e) => e\n}\nmatch math.sqrt(9.0) {\n  case Ok(x) => \"{x}/{digest}/{p}\"\n  case Err(e) => e\n}",
        )],
    ),
    // v5.4 `std.fs`/`std.io`: module wrappers over the existing host boundary.
    // The harness seeds `input.txt`; file state and stdout are compared after
    // both interpreter and emitted runs, so read/write/print stay byte-identical.
    (
        "mod_std_fs_io",
        "main.tpz",
        &[(
            "main.tpz",
            "import std.fs\nimport std.io\nimport std.path\nlet inputPath = match path.from(\"input.txt\") {\n  case Ok(value) => value\n  case Err(_) => \"input.txt\"\n}\nlet before = match fs.readText(inputPath) {\n  case Ok(text) => text\n  case Err(e) => e\n}\nlet wrote = match fs.writeText(inputPath, \"world\") {\n  case Ok(_) => \"ok\"\n  case Err(e) => e\n}\nlet after = match fs.readText(inputPath) {\n  case Ok(text) => text\n  case Err(e) => e\n}\nio.writeLine(\"{before}/{wrote}/{after}\")\nafter",
        )],
    ),
    // v5.4 `std.test`: assertion helpers return/unwrap values or fault through the
    // shared Test namespace. The harness seeds `input.txt` as `hello`, so
    // assertGolden also proves the host read path stays run≡build.
    (
        "mod_std_test_assertions",
        "main.tpz",
        &[(
            "main.tpz",
            "import std.test { assertEq, assertNe, assertContains, assertOk, assertErr, assertSome, assertNone, assertGolden }\nlet ok = assertOk(Ok(41))\nlet err = assertErr(Err(\"bad\"))\nlet some = assertSome(Some(1))\nassertEq(ok + some, 42)\nassertNe(err, \"good\")\nassertContains(\"topaz\", \"opa\")\nassertNone(None)\nassertGolden(\"input.txt\", \"hello\")\n\"{ok}/{err}/{some}\"",
        )],
    ),
    // v5.4 `std.gen` + `std.test`: deterministic finite generators and the
    // monomorphic source-order property runner API that future `property`
    // syntax can lower to.
    (
        "mod_std_gen_property_helpers",
        "main.tpz",
        &[(
            "main.tpz",
            "import std.gen\nimport std.test { assertEq, forAllBool, forAllInt }\nlet values = gen.intRange(-2, 2)\nlet mut total = 0\nlet mut seen = \"\"\nforAllInt(\"sum\", values, (x) => {\n  total += x\n  seen = \"{seen},{x}\"\n  assertEq(x * x >= 0, true)\n})\nlet mut truthy = 0\nlet mut boolSeen = \"\"\nforAllBool(\"bools\", gen.bools(), (flag) => {\n  boolSeen = \"{boolSeen},{flag}\"\n  if flag { truthy += 1 }\n})\n\"{values.length}/{total}/{gen.intRange(3, 1).length}/{truthy}/{seen}/{boolSeen}\"",
        )],
    ),
    // v5.4 `std.encoding`: module wrappers over the public `Encoding` facade and
    // the same Bytes codec leaves.
    (
        "mod_std_encoding",
        "main.tpz",
        &[(
            "main.tpz",
            "import std.encoding\nlet b = encoding.utf8Encode(\"topaz\")\nlet hex = encoding.hexEncode(b)\nlet b64 = encoding.base64Encode(b)\nlet text = match encoding.base64Decode(b64).flatMap((bytes) => encoding.utf8Decode(bytes)) {\n  case Ok(value) => value\n  case Err(e) => e\n}\n\"{hex}/{b64}/{text}\"",
        )],
    ),
    // v5.4 `std.parser`: pure scanner helpers over scalar indices. This keeps
    // parser/transpiler helpers in Topaz source instead of adding another shared
    // Rust leaf, and pins negative-start clamping plus non-ASCII rejection.
    (
        "mod_std_parser_scanners",
        "main.tpz",
        &[(
            "main.tpz",
            "import std.parser\nlet text = \"  abc123-한\"\nlet tokenStart = parser.takeWhileAsciiWhitespace(text, -4)\nlet alphaEnd = parser.takeWhileAsciiAlpha(text, tokenStart)\nlet tokenEnd = parser.takeWhileAsciiAlnum(text, tokenStart)\n\"{tokenStart}/{alphaEnd}/{tokenEnd}/{parser.isAsciiAlpha(text, 9)}/{parser.isAsciiDigit(text, 5)}\"",
        )],
    ),
    // v5.4 practical data-tool modules: the public `std.*` wrappers must lower as
    // ordinary module records, not only through the legacy prelude namespaces.
    (
        "mod_std_data_tools",
        "main.tpz",
        &[(
            "main.tpz",
            r#"import std.path
import std.hash
import std.regex
import std.csv
import std.toml
import std.json
import std.url
import std.cli
import std.encoding
import std.parser
import std.date
import std.bigint
import std.decimal

function cell(rows: Array<Array<string>>, row: int, col: int) -> string {
  match rows.get(row).flatMap((values) => values.get(col)) {
    case Some(value) => value
    case None => "none"
  }
}

function parseJsonText(text: string) -> Result<JSONValue, string> {
  match json.parse(text) {
    case Ok(value) => Ok(value)
    case Err(e) => Err(e.message)
  }
}

function demo() -> Result<string, string> {
  let p = path.from("src//./main.tpz")?
  let joinedPath = path.join(path.from("src")?, "lib/util.tpz")?
  let pathParent = match path.parent(p) {
    case Some(value) => path.toString(value)
    case None => "none"
  }
  let pathFile = match path.fileName(p) {
    case Some(value) => value
    case None => "none"
  }
  let pathExt = match path.extension(p) {
    case Some(value) => value
    case None => "none"
  }
  let pathMd = path.withExtension(p, "md")?
  let pathSummary = "{path.toString(p)}:{path.toString(joinedPath)}:{pathParent}:{pathFile}:{pathExt}:{path.toString(path.normalize(pathMd))}"
  let digest = hash.sha256(encoding.utf8Encode("abc")).toHex()
  let digestTextOk = hash.sha256Text("abc") == digest
  let digest512Text = hash.sha512Text("abc")
  let re = regex.compile("\\d+")?
  let found = match re.find("챕12") {
    case Some(m) => "{m.start}/{m.end}/{m.text}"
    case None => "none"
  }
  let foundViaModule = match regex.find(re, "x7") {
    case Some(m) => "{regex.start(m)}-{regex.end(m)}-{regex.textOf(m)}-{regex.groups(m).length}-{regex.named(m).keys.length}"
    case None => "none"
  }
  let matches = regex.findAll(re, "a1 b22").map((m) => m.text).join("+")
  let pieces = regex.split(regex.compile(",\\s*")?, "a, b,c").join("/")
  let replaced = regex.replaceAll(re, "a1b22", "X")
  let regexMatched = regex.isMatch(re, "abc123")
  let regexSummary = "{regexMatched}/{found}/{foundViaModule}/{matches}/{pieces}/{replaced}"
  let rows = csv.parse("name,age\nAda,36")?
  let quoted = csv.stringify([["a,b", "c\"d"]])
  let tomlValue = toml.parse("name = \"Ada\"\n[db]\nport = 5432")?
  let tomlJson = json.stringify(toml.toJson(tomlValue))?
  let tomlJsonValue = toml.toJson(tomlValue)
  let jsonKeys = match json.keys(tomlJsonValue) {
    case Some(keys) => keys.join("+")
    case None => "none"
  }
  let jsonLen = match json.length(tomlJsonValue) {
    case Some(value) => "{value}"
    case None => "none"
  }
  let jsonName = match json.get(tomlJsonValue, "name").flatMap((value) => json.asString(value)) {
    case Some(value) => value
    case None => "none"
  }
  let jsonDb = match json.get(tomlJsonValue, "db") {
    case Some(value) => value
    case None => tomlJsonValue
  }
  let jsonPort = match json.get(jsonDb, "port").flatMap((value) => json.numberText(value)) {
    case Some(value) => value
    case None => "none"
  }
  let jsonArr = parseJsonText("[1,true,null]")?
  let jsonFirst = match json.at(jsonArr, 0).flatMap((value) => json.asInt(value)) {
    case Some(value) => "{value}"
    case None => "none"
  }
  let jsonSecond = match json.at(jsonArr, 1).flatMap((value) => json.asBool(value)) {
    case Some(value) => "{value}"
    case None => "none"
  }
  let jsonThirdNull = match json.at(jsonArr, 2) {
    case Some(value) => json.isNull(value)
    case None => false
  }
  let jsonArrLen = match json.asArray(jsonArr) {
    case Some(values) => "{values.length}"
    case None => "none"
  }
  let jsonSummary = "{json.kind(tomlJsonValue)}:{jsonKeys}:{jsonLen}:{jsonName}:{jsonPort}:{jsonFirst}:{jsonSecond}:{jsonThirdNull}:{jsonArrLen}"
  let parsedUrl = url.parse("HTTPS://Example.COM/a?q=topaz&tag=a&tag=b#frag")?
  let host = match url.host(parsedUrl) {
    case Some(value) => value
    case None => "none"
  }
  let tags = match url.query(parsedUrl).get("tag") {
    case Some(values) => values.join("+")
    case None => "none"
  }
  let fragment = match url.fragment(parsedUrl) {
    case Some(value) => value
    case None => "none"
  }
  let args = ["--out", "file", "input", "--", "--literal"]
  let out = match cli.option(args, "--out") {
    case Some(value) => value
    case None => "none"
  }
  let positionals = cli.positionals(args).join(",")
  let tokenEnd = parser.takeWhileAsciiAlnum("  abc123-한", 2)
  let d = date.fromYmd(2024, 2, 29)?
  let next = date.addDays(d, 1)
  let bigValue = match bigint.parse("ff", 16) {
    case Some(value) => value
    case None => bigint.fromInt(0)
  }
  let bigInt = match bigint.asInt(bigValue) {
    case Some(value) => "{value}"
    case None => "none"
  }
  let bigDiv = match bigint.div(bigValue, bigint.fromInt(10)) {
    case Ok(value) => bigint.toString(value)
    case Err(e) => e
  }
  let bigMod = match bigint.modulo(bigValue, bigint.fromInt(10)) {
    case Ok(value) => bigint.toString(value)
    case Err(e) => e
  }
  let big = "{bigint.toString(bigValue, 10)}/{bigInt}/{bigDiv}/{bigMod}"
  let decValue = match decimal.parse("12.3400") {
    case Some(value) => value
    case None => decimal.fromInt(0)
  }
  let decInt = match decimal.asInt(decValue) {
    case Some(value) => "{value}"
    case None => "none"
  }
  let decDiv = match decimal.div(decimal.fromInt(5), decimal.fromInt(2), 1) {
    case Ok(value) => decimal.toString(value)
    case Err(e) => e
  }
  let decHalf = match decimal.parse("2.5") {
    case Some(value) => decimal.toString(decimal.roundWithMode(value, 0, RoundingMode.HalfUp))
    case None => "none"
  }
  let decDivMode = match decimal.divWithMode(decimal.fromInt(1), decimal.fromInt(8), 2, RoundingMode.HalfUp) {
    case Ok(value) => decimal.toString(value)
    case Err(e) => e
  }
  let dec = "{decimal.toString(decValue)}/{decimal.scale(decValue)}/{decInt}/{decimal.toString(decimal.round(decValue, 1))}/{decDiv}/{decHalf}/{decDivMode}"
  Ok("{pathSummary}|{digest}|{digestTextOk}|{digest512Text}|{regexSummary}|{cell(rows, 1, 0)}|{quoted}|{tomlJson}|{jsonSummary}|{url.scheme(parsedUrl)}:{host}:{url.path(parsedUrl)}:{tags}:{fragment}:{url.toString(parsedUrl)}|{out}:{positionals}|{tokenEnd}|{date.toIso(next)}:{date.year(next)}:{date.month(next)}:{date.day(next)}|{big}|{dec}")
}

match demo() {
  case Ok(line) => line
  case Err(error) => "error: {error}"
}"#,
        )],
    ),
    // v5.4 `std.codec`: deterministic compression wrappers over the shared Codec leaves.
    (
        "mod_std_codec_gzip",
        "main.tpz",
        &[(
            "main.tpz",
            "import std.codec\nimport std.encoding\nlet raw = encoding.utf8Encode(\"topaz\")\nlet gz = match codec.gzipCompress(raw) {\n  case Ok(bytes) => bytes\n  case Err(e) => encoding.utf8Encode(e)\n}\nlet df = match codec.deflateCompress(raw) {\n  case Ok(bytes) => bytes\n  case Err(e) => encoding.utf8Encode(e)\n}\nlet zs = match codec.zstdCompress(raw) {\n  case Ok(bytes) => bytes\n  case Err(e) => encoding.utf8Encode(e)\n}\nlet a = match codec.gzipDecompress(gz).flatMap((bytes) => encoding.utf8Decode(bytes)) {\n  case Ok(text) => text\n  case Err(e) => e\n}\nlet b = match codec.deflateDecompress(df).flatMap((bytes) => encoding.utf8Decode(bytes)) {\n  case Ok(text) => text\n  case Err(e) => e\n}\nlet c = match codec.zstdDecompress(zs).flatMap((bytes) => encoding.utf8Decode(bytes)) {\n  case Ok(text) => text\n  case Err(e) => e\n}\n\"{a}/{b}/{c}\"",
        )],
    ),
    // v5.4 `std.http`: deterministic HTTP request/response value types only.
    // Topaz opens no sockets; handlers receive explicit values and return explicit
    // values. This fixture pins exported-record type propagation across the virtual
    // module, field access in the importer, and the pure header/text helpers.
    (
        "mod_std_http_values",
        "main.tpz",
        &[(
            "main.tpz",
            r#"import std.http
record HealthPayload derives JSON { ok: bool }
match URL.parse("https://example.com/health?tag=a&tag=b") {
  case Ok(url) => {
    let req = http.request("GET", url, map { "accept": ["text/plain", "application/json"] }, Bytes.encodeUtf8(""))
    let accept = match http.header(req, "accept") {
      case Some(value) => value
      case None => "none"
    }
    let missing = match http.header(req, "missing") {
      case Some(value) => value
      case None => "none"
    }
    let resp = http.text(200, "ok")
    let ctype = match resp.headers.get("content-type").flatMap((values) => values.get(0)) {
      case Some(value) => value
      case None => "none"
    }
    let body = match resp.body.decodeUtf8() {
      case Ok(value) => value
      case Err(e) => e
    }
    let jsonResp = match http.json(201, HealthPayload { ok: true }) {
      case Ok(value) => value
      case Err(e) => http.text(500, e)
    }
    let jsonType = match jsonResp.headers.get("content-type").flatMap((values) => values.get(0)) {
      case Some(value) => value
      case None => "none"
    }
    let jsonBody = match jsonResp.body.decodeUtf8() {
      case Ok(value) => value
      case Err(e) => e
    }
    "{req.method}/{req.url.path()}/{accept}/{missing}/{resp.status}/{ctype}/{body}/{jsonResp.status}/{jsonType}/{jsonBody}"
  }
  case Err(e) => e
}"#,
        )],
    ),
    // §17 selected imports may be type-only (`export type` / `export record`).
    // They feed the checker but do not bind runtime values; the selected function
    // remains the only imported value.
    // v5.4 Web Target library layer: `std.dom` is a pure value surface over the
    // deterministic event-in/command-out trace. No browser host methods are added;
    // command values render through ordinary Topaz values. The JSON trace aliases
    // preserve the first wire shape while the public helpers are generic over the
    // application's message type.
    (
        "mod_std_dom_trace",
        "main.tpz",
        &[(
            "main.tpz",
            r##"import std.dom
function render(view: dom.TraceHtml, commands: Array<dom.TraceCommand>) -> string {
  "{view}|{commands.join(";")}"
}
function makeView(click: dom.TraceEvent, label: dom.TraceHtml) -> dom.TraceHtml {
  dom.element<dom.JSONValue>("button", [dom.attr("id", "save"), dom.attr("type", "button")], [click], [label])
}
function demo() -> Result<string, string> {
  let msg = match JSON.parse("true") {
    case Ok(value) => value
    case Err(error) => return Err(error.message)
  }
  let label = dom.text<dom.JSONValue>("Save")
  let click = dom.event<dom.JSONValue>("click", msg)
  let view = makeView(click, label)
  let setStatus = dom.setText<dom.JSONValue>("#status", "saved")
  let markOk = dom.addClass<dom.JSONValue>("#status", "ok")
  let focusSave = dom.focus<dom.JSONValue>("#save")
  let bounce = dom.dispatch<dom.JSONValue>(msg)
  let commands = [setStatus, markOk, focusSave, bounce]
  Ok(render(view, commands))
}
match demo() {
  case Ok(line) => line
  case Err(error) => "error: {error}"
}"##,
        )],
    ),
    (
        "mod_std_dom_generic_helpers",
        "main.tpz",
        &[(
            "main.tpz",
            r##"import std.dom
enum Msg derives Show {
  SaveClicked,
  Toggle(bool),
}
function view() -> dom.Html<Msg> {
  let label = dom.text<Msg>("Save")
  dom.element<Msg>("button", [dom.attr("id", "save"), dom.attr("type", "button")], [dom.event<Msg>("click", Msg.SaveClicked)], [label])
}
function update(msg: Msg) -> Array<dom.Command<Msg>> {
  let setStatus = dom.setText<Msg>("#status", "{msg}")
  let markOk = dom.addClass<Msg>("#status", "ok")
  let bounce = dom.dispatch<Msg>(msg)
  [setStatus, markOk, bounce]
}
let html = view()
let commands = update(Msg.Toggle(true))
"{html}|{commands.join(";")}"
"##,
        )],
    ),
    (
        "mod_selected_type_only_exports",
        "main.tpz",
        &[
            (
                "types.tpz",
                "export type Id = int\nexport record Box derives Show { value: int }\nexport function make(value: Id) -> Box { Box { value: value } }\n",
            ),
            (
                "main.tpz",
                "import types { Id, Box, make }\nlet id = 7\nlet b = make(id)\n\"{id}/{b.value}\"",
            ),
        ],
    ),
    // v5.4 generic nominal exports across a namespace import. The checker tests
    // pin namespace-qualified annotations; this fixture pins the runtime module
    // shape so interpreter and emitted module records still agree on the values.
    (
        "mod_generic_nominal_qualified_types",
        "main.tpz",
        &[
            (
                "model.tpz",
                "export record Box<T> {\n  value: T,\n}\nexport enum Maybe<T> {\n  Missing,\n  Present(T),\n}\nexport newtype Id<T> = T\nexport function makeBox(value: int) -> Box<int> {\n  Box { value: value }\n}\nexport function makeMaybe(value: int) -> Maybe<int> {\n  Maybe.Present(value)\n}\nexport function makeId(value: string) -> Id<string> {\n  Id(value)\n}\nexport function render(b: Box<int>, maybe: Maybe<int>, id: Id<string>) -> string {\n  let picked = match maybe {\n    case Present(value) => value\n    case Missing => 0\n  }\n  \"{b.value}:{picked}:{id.value()}\"\n}\n",
            ),
            (
                "main.tpz",
                "import model\nlet b = model.makeBox(7)\nlet maybe = model.makeMaybe(5)\nlet id = model.makeId(\"u-42\")\nmodel.render(b, maybe, id)",
            ),
        ],
    ),
    // v5.4 qualified generic nominal type patterns: the type arguments belong to
    // the importer source while the nominal payload/base declarations belong to
    // `model.tpz`. Both engines must test direct generic payloads under the correct
    // source, not just by nominal id.
    (
        "mod_qualified_generic_nominal_type_patterns",
        "main.tpz",
        &[
            (
                "model.tpz",
                "export record Box<T> {\n  value: T,\n}\nexport enum Maybe<T> {\n  Missing,\n  Present(T),\n}\nexport newtype Id<T> = T\nexport function makeBox(value: int) -> Box<int> {\n  Box { value: value }\n}\nexport function makeMaybe(value: int) -> Maybe<int> {\n  Maybe.Present(value)\n}\nexport function makeId(value: string) -> Id<string> {\n  Id(value)\n}\n",
            ),
            (
                "main.tpz",
                "import model\nlet b = model.makeBox(7)\nlet maybe = model.makeMaybe(5)\nlet id = model.makeId(\"u-42\")\nlet boxHit = match b {\n  case x: model.Box<int> => x.value\n  case _ => 0\n}\nlet picked = match maybe {\n  case m: model.Maybe<int> => 5\n  case _ => -1\n}\nlet idHit = match id {\n  case x: model.Id<string> => x.value()\n  case _ => \"id-miss\"\n}\nlet boxMixed: model.Box<int> | string = b\nlet boxMiss = match boxMixed {\n  case missBox: model.Box<string> => \"bad\"\n  case _ => \"miss\"\n}\nlet maybeMixed: model.Maybe<int> | string = maybe\nlet maybeMiss = match maybeMixed {\n  case missMaybe: model.Maybe<string> => \"bad\"\n  case _ => \"miss\"\n}\nlet idMixed: model.Id<string> | int = id\nlet idMiss = match idMixed {\n  case missId: model.Id<int> => \"bad\"\n  case _ => \"miss\"\n}\n\"{boxHit}:{picked}:{idHit}:{boxMiss}:{maybeMiss}:{idMiss}\"",
            ),
        ],
    ),
    // v5.4 generic nominal pass-through: a generic function may return a
    // record/enum/newtype instance carrying its own type parameter, and the
    // call-site context solves that parameter for both interpreter and emit.
    (
        "generic_nominal_pass_through",
        "main.tpz",
        &[(
            "main.tpz",
            "record Html<Msg> {\n  message: Msg,\n}\nenum Maybe<T> {\n  Missing,\n  Present(T),\n}\nnewtype Id<T> = T\nfunction html<T>(value: T) -> Html<T> {\n  Html { message: value }\n}\nfunction just<T>(value: T) -> Maybe<T> {\n  Maybe.Present(value)\n}\nfunction none<T>() -> Maybe<T> {\n  Maybe.Missing\n}\nfunction ident<T>(value: T) -> Id<T> {\n  Id(value)\n}\nlet view: Html<int> = html(7)\nlet maybe: Maybe<int> = just(view.message)\nlet empty: Maybe<string> = none()\nlet id: Id<string> = ident(\"ok\")\nlet picked = match maybe {\n  case Present(value) => value\n  case Missing => 0\n}\nlet fallback = match empty {\n  case Present(value) => value\n  case Missing => \"missing\"\n}\n\"{view.message}:{picked}:{fallback}:{id.value()}\"",
        )],
    ),
    // Namespace import of a multi-segment module exporting a function, a `let`,
    // and a `const`; the entry interpolates all three through the bound record.
    (
        "mod_namespace",
        "main.tpz",
        &[
            (
                "utils/strings.tpz",
                "export function shout(s: string) -> string {\n    return \"{s}!\"\n}\nexport let greeting = \"hello\"\nexport const N = 3\n",
            ),
            (
                "main.tpz",
                "import utils.strings\n\"{strings.greeting}|{strings.shout(strings.greeting)}|{strings.N}\"",
            ),
        ],
    ),
    // `export type` erases at runtime: a non-entry module exports a type ALONGSIDE a
    // function; via a NAMESPACE import the type contributes no record field — emit
    // must skip it, not refuse. run≡build pins the erasure.
    (
        "mod_export_type_namespace",
        "main.tpz",
        &[
            (
                "typedlib.tpz",
                "export type Id = int\nexport function id(x: int) -> int {\n    return x\n}\n",
            ),
            ("main.tpz", "import typedlib\ntypedlib.id(42)"),
        ],
    ),
    // §17 a TYPE-ONLY module (its ONLY export is a type, which erases) has zero
    // RUNTIME exports, yet the resolver accepts the import (it exports a name) and
    // the interpreter binds an empty namespace. The emitter must build an empty
    // `Value::record([])` rather than refuse `module exports nothing` — run≡build
    // pins that a type-only namespace import compiles and runs. (A truly
    // export-less module is rejected earlier by the resolver, TPZ3010.)
    (
        "mod_type_only_namespace",
        "main.tpz",
        &[
            ("idtype.tpz", "export type Id = int\n"),
            ("main.tpz", "import idtype\n0"),
        ],
    ),
    // §17 QUALIFIED type `m.Id` at MODULE TOP: the typed `let` runs the SAME
    // conformance test in both engines (emit resolves the exported alias to its
    // body via the cross-module `TypeCtx`; the interpreter via the namespace
    // binding + the exporting module's alias table) — run≡build pins the resolve.
    (
        "mod_qualified_scalar",
        "main.tpz",
        &[
            ("idtype.tpz", "export type Id = int\n"),
            ("main.tpz", "import idtype\nlet x: idtype.Id = 5\nx\n"),
        ],
    ),
    // §17 a STRUCTURAL exported alias body (`{ id: int }`) crossing the boundary.
    (
        "mod_qualified_record",
        "main.tpz",
        &[
            ("rowmod.tpz", "export type Row = { id: int }\n"),
            (
                "main.tpz",
                "import rowmod\nlet r: rowmod.Row = { id: 7 }\nr.id\n",
            ),
        ],
    ),
    // §17 qualified type in a TOP-LEVEL FUNCTION body — the bounded slice's key
    // case: the body's use-site emit locals (params, not the un-captured namespace)
    // plus the module-top namespace map decide the head soundly (`in_nested` false).
    (
        "mod_qualified_in_fn",
        "main.tpz",
        &[
            ("idtype.tpz", "export type Id = int\n"),
            (
                "main.tpz",
                "import idtype\nfunction pick(n: int) -> int {\n    let x: idtype.Id = n\n    return x\n}\npick(9)\n",
            ),
        ],
    ),
    // §17 a qualified element type inside a structural container (`Array<m.Id>`):
    // the qualified resolves to the inner element test of the array check.
    (
        "mod_qualified_array",
        "main.tpz",
        &[
            ("idtype.tpz", "export type Id = int\n"),
            (
                "main.tpz",
                "import idtype\nlet xs: Array<idtype.Id> = [1, 2]\nxs\n",
            ),
        ],
    ),
    // Section 17 generic qualified structural alias crossing the boundary. The
    // alias body fields live in the exporting module; the type arguments come
    // from the importer source.
    (
        "mod_qualified_generic_alias",
        "main.tpz",
        &[
            ("pairs.tpz", "export type Pair<T> = { a: T, b: T }\n"),
            (
                "main.tpz",
                "import pairs\nlet p: pairs.Pair<int> = { a: 1, b: 2 }\nlet hit = match p {\n  case x: pairs.Pair<int> => x.a + x.b\n  case _ => 0\n}\nlet mixed: pairs.Pair<int> | string = p\nlet miss = match mixed {\n  case bad: pairs.Pair<string> => \"bad\"\n  case _ => \"miss\"\n}\n\"{hit}:{miss}\"",
            ),
        ],
    ),
    // §17 the SAME member name exported by TWO modules resolves to each module's
    // OWN body (`a.Id` = int, `b.Id` = string) — no cross-talk in the namespace map.
    (
        "mod_qualified_two_modules",
        "main.tpz",
        &[
            ("amod.tpz", "export type Id = int\n"),
            ("bmod.tpz", "export type Id = string\n"),
            (
                "main.tpz",
                "import amod\nimport bmod\nlet x: amod.Id = 1\nlet y: bmod.Id = \"hi\"\n\"{x}-{y}\"\n",
            ),
        ],
    ),
    // §9 an optional-pipe MUTATOR rooted at a NAMESPACE member (`lib.xs`, an immutable
    // exported `let`) must fault GUARD_IMMUTABLE in BOTH engines — the immutable-root
    // arm includes `Bind::Namespace`/`TopFnCell`, not just `Imm`/`ImmCell`.
    (
        "mod_optional_pipe_mutator_namespace_immutable",
        "main.tpz",
        &[
            ("lib.tpz", "export let xs = [1]\n"),
            ("main.tpz", "import lib\n2 |> lib.xs?.push()\nlib.xs\n"),
        ],
    ),
    // Same erasure via a SELECTED import (`import typedlib { id }`): the runtime
    // export resolves while the exported type stays erased.
    (
        "mod_export_type_selected",
        "main.tpz",
        &[
            (
                "typedlib.tpz",
                "export type Id = int\nexport function id(x: int) -> int {\n    return x\n}\n",
            ),
            ("main.tpz", "import typedlib { id }\nid(42)"),
        ],
    ),
    // Selected import with an alias (`inc as bump`) plus a plain selected `let`.
    (
        "mod_selected_alias",
        "main.tpz",
        &[
            (
                "lib.tpz",
                "export function inc(x: int) -> int {\n    return x + 1\n}\nexport let base = 10\n",
            ),
            ("main.tpz", "import lib { inc as bump, base }\nbump(base)"),
        ],
    ),
    // Transitive diamond: `left` and `right` both import the shared `base`
    // (built once); the entry imports both. Pins transitive + diamond seeding.
    (
        "mod_transitive_diamond",
        "main.tpz",
        &[
            ("base.tpz", "export let v = 5\n"),
            ("left.tpz", "import base\nexport let l = base.v + 1\n"),
            ("right.tpz", "import base\nexport let r = base.v + 2\n"),
            ("main.tpz", "import left\nimport right\nleft.l + right.r"),
        ],
    ),
    // A SELECTED import INSIDE a non-entry module (`mid` imports `inc` from
    // `base` and calls it in its own exported function) — a distinct emit
    // branch from the entry's import seeding.
    (
        "mod_module_selected_import",
        "main.tpz",
        &[
            (
                "base.tpz",
                "export function inc(x: int) -> int {\n    return x + 1\n}\n",
            ),
            (
                "mid.tpz",
                "import base { inc }\nexport function bump(n: int) -> int {\n    return inc(n)\n}\n",
            ),
            ("main.tpz", "import mid\nmid.bump(41)"),
        ],
    ),
    // A fault DURING a NON-ENTRY module's initialization: `lib`'s top-level
    // `let` calls `f("s")`, which the parameter guard rejects while `lib`
    // initializes (before the entry runs). Pins TWO things at once — (a) the
    // boundary guard fires across a module boundary, and (b) the interpreter's
    // `run_unit` import-chain suffix and the emitter's module-record wrapper
    // produce the BYTE-IDENTICAL message `… (during initialization of module
    // \`lib\`; import chain: main -> lib)` (run == build on the wrapped fault).
    (
        "mod_init_boundary_fault",
        "main.tpz",
        &[
            (
                "lib.tpz",
                "export function f(x: int) -> int {\n    return x\n}\nexport let bad = f(\"s\")\n",
            ),
            ("main.tpz", "import lib\nlib.bad"),
        ],
    ),
    // §7 forward-ref recursion in a NON-ENTRY module (TopFnCell there too): `a`
    // names the later `b` across a non-function statement; the exported function is
    // read out of its filled top cell into the module record — run≡build.
    (
        "mod_forward_ref_non_entry",
        "main.tpz",
        &[
            (
                "lib.tpz",
                "export function a(n: int) -> int {\n    b(n)\n}\nlet sep = 1\nexport function b(n: int) -> int {\n    n * 2\n}\n",
            ),
            ("main.tpz", "import lib\nlib.a(5)"),
        ],
    ),
    // Typed JSON materializes aliases in the called closure's defining
    // module. The entry deliberately declares the same short name with a
    // different body after `lib` initializes; run≡build must still decode int.
    (
        "mod_typed_json_defining_alias_scope",
        "main.tpz",
        &[
            (
                "lib.tpz",
                "type Scalar = int\nexport function decodeLocal() -> int {\n    match JSON.parseAs<Scalar>(\"7\") {\n    case Ok(value) => value\n    case Err(_) => 0\n    }\n}\n",
            ),
            (
                "main.tpz",
                "import lib\ntype Scalar = string\nlib.decodeLocal()\n",
            ),
        ],
    ),
    (
        "mod_nominal_record_mutable_default_defining_scope",
        "main.tpz",
        &[
            (
                "model.tpz",
                "let mut base = 36\nexport function setBase(value: int) -> unit {\n    base = value\n}\nexport record User { age: int = base }\n",
            ),
            (
                "main.tpz",
                "import model { User, setBase }\nsetBase(41)\nlet user = User {}\nuser.age",
            ),
        ],
    ),
];

pub(crate) const VERSIONED_MODULE_FIXTURES: &[VersionedModuleFixtureDef] = &[(
    "v520_module_stable_nominals_and_imported_typed_json",
    "main.tpz",
    LangVersion::V5_20,
    &[
        (
            "main.tpz",
            r#"import alpha { User as AlphaUser, Code as AlphaCode, Flag as AlphaFlag }
import beta { User as BetaUser, Code as BetaCode, Flag as BetaFlag }
import model
import selected { Box, UserAlias }
let qualified = match JSON.parseAs<model.User>("\{\"name\":\"Ada\"\}") {
    case Ok(user) => user.rank + 1
    case Err(_) => 0
}
let aliased = match JSON.parseAs<UserAlias>("\{\"name\":\"Bea\"\}") {
    case Ok(user) => if user.name == "Bea" { 1 } else { 0 }
    case Err(_) => 0
}
let generic = match JSON.parseAs<Box<int>>("\{\"value\":7,\"rank\":8\}") {
    case Ok(boxed) => boxed.value + boxed.rank
    case Err(_) => 0
}
let alpha = AlphaUser { id: 1 }
let beta = BetaUser { id: 1 }
let alphaCode = AlphaCode(1)
let betaCode = BetaCode(1)
let alphaFlag = AlphaFlag.On
let betaFlag = BetaFlag.On
let users = Set.of(alpha, beta)
let mut labels = Map.new()
labels.insert(alpha, "alpha")
labels.insert(beta, "beta")
let recordPattern = match alpha {
    case BetaUser { id } => 9
    case AlphaUser { id } => id
}
let codePattern = match alphaCode {
    case BetaCode(value) => 9
    case AlphaCode(value) => value
}
let flagPattern = match alphaFlag {
    case value: BetaFlag => 9
    case value: AlphaFlag => 1
}
let collisions = (if alpha == beta { 100 } else { 0 })
    + (if alphaCode == betaCode { 1000 } else { 0 })
    + (if alphaFlag == betaFlag { 10000 } else { 0 })
qualified + aliased + generic + collisions + users.length * 10 + labels.length + recordPattern + codePattern + flagPattern"#,
        ),
        (
            "alpha.tpz",
            "export record User { id: int }\nexport newtype Code = int\nexport enum Flag { On }\n",
        ),
        (
            "beta.tpz",
            "export record User { id: int }\nexport newtype Code = int\nexport enum Flag { On }\n",
        ),
        (
            "scalar.tpz",
            "export type Scalar = int\nexport record Hidden { name: string }\n",
        ),
        (
            "model.tpz",
            "import scalar { Scalar }\nexport record User { name: string, rank: Scalar = 0 }\n",
        ),
        (
            "selected.tpz",
            "import scalar { Scalar, Hidden }\nexport type UserAlias = Hidden\nexport record Box<T> { value: T, rank: Scalar }\n",
        ),
    ],
)];

pub(crate) const EXTERN_MODULE_FIXTURES: &[ExternModuleFixtureDef] = &[
    (
        "mod_extern_replay_positive",
        "main.tpz",
        &[(
            "main.tpz",
            r#"import host.math { twice }
let answer = twice(21)
print("twice={answer}")
answer"#,
        )],
        &[(
            "host.math",
            "host/math.tpz",
            "export function twice(x: int) -> int { x }",
            None,
        )],
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"42"}}"#,
    ),
    (
        "mod_extern_replay_missing_row_fault",
        "main.tpz",
        &[("main.tpz", "import host.math { twice }\ntwice(22)")],
        &[(
            "host.math",
            "host/math.tpz",
            "export function twice(x: int) -> int { x }",
            None,
        )],
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"42"}}"#,
    ),
    (
        "mod_extern_replay_bytes_result_abi",
        "main.tpz",
        &[(
            "main.tpz",
            r#"import host.codec { echoBytes }
let bytes = Bytes.encodeUtf8("hi")
match echoBytes(bytes) {
  case Ok(value) => value.toHex()
  case Err(e) => e
}"#,
        )],
        &[(
            "host.codec",
            "host/codec.tpz",
            "export function echoBytes(value: Bytes) -> Result<Bytes, string> { Ok(Bytes.empty()) }",
            None,
        )],
        r#"{"module":"host.codec","function":"echoBytes","args":[{"$":"bytes","hex":"6869"}],"result":{"$":"ok","value":{"$":"bytes","hex":"6869"}}}"#,
    ),
];
