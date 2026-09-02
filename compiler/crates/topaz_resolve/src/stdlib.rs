//! Virtual v5.4 `std.*` module sources.
//!
//! The actual pure leaves still live in the shared builtin/runtime tables; these
//! modules are the §17 import surface that lets package code say `import
//! std.math` instead of relying on prelude-style global namespaces.

type StdModule = (&'static str, &'static str, &'static str);

const MODULES: &[StdModule] = &[
    ("std.math", "std/math.tpz", STD_MATH),
    ("std.bytes", "std/bytes.tpz", STD_BYTES),
    ("std.hash", "std/hash.tpz", STD_HASH),
    ("std.cli", "std/cli.tpz", STD_CLI),
    ("std.path", "std/path.tpz", STD_PATH),
    ("std.fs", "std/fs.tpz", STD_FS),
    ("std.io", "std/io.tpz", STD_IO),
    ("std.regex", "std/regex.tpz", STD_REGEX),
    ("std.csv", "std/csv.tpz", STD_CSV),
    ("std.toml", "std/toml.tpz", STD_TOML),
    ("std.json", "std/json.tpz", STD_JSON),
    ("std.url", "std/url.tpz", STD_URL),
    ("std.http", "std/http.tpz", STD_HTTP),
    ("std.dom", "std/dom.tpz", STD_DOM),
    ("std.encoding", "std/encoding.tpz", STD_ENCODING),
    ("std.codec", "std/codec.tpz", STD_CODEC),
    ("std.date", "std/date.tpz", STD_DATE),
    ("std.bigint", "std/bigint.tpz", STD_BIGINT),
    ("std.decimal", "std/decimal.tpz", STD_DECIMAL),
    ("std.gen", "std/gen.tpz", STD_GEN),
    ("std.parser", "std/parser.tpz", STD_PARSER),
    ("std.test", "std/test.tpz", STD_TEST),
];

pub(crate) fn module_source(segments: &[&str]) -> Option<(&'static str, &'static str)> {
    let name = match segments {
        ["std", name] => *name,
        _ => return None,
    };
    MODULES
        .iter()
        .find(|module| module.0.strip_prefix("std.") == Some(name))
        .map(|module| (module.1, module.2))
}

pub(crate) fn module_identities() -> impl Iterator<Item = &'static str> {
    MODULES.iter().map(|module| module.0)
}

const STD_MATH: &str = r#"
export function sqrt(x: float) -> Result<float, string> { Math.sqrt(x) }
export function abs(x: float) -> float { Math.abs(x) }
export function floor(x: float) -> float { Math.floor(x) }
export function ceil(x: float) -> float { Math.ceil(x) }
export function round(x: float) -> float { Math.round(x) }
export function sin(x: float) -> float { Math.sin(x) }
export function cos(x: float) -> float { Math.cos(x) }
export function tan(x: float) -> float { Math.tan(x) }
export function isNaN(x: float) -> bool { Math.isNaN(x) }
export function isFinite(x: float) -> bool { Math.isFinite(x) }
export function parseFloat(s: string) -> Result<float, string> { Math.parseFloat(s) }
export function min(a: float, b: float) -> float { Math.min(a, b) }
export function max(a: float, b: float) -> float { Math.max(a, b) }
"#;

const STD_BYTES: &str = r#"
export type Bytes = Bytes
export type ByteBuffer = ByteBuffer
export function empty() -> Bytes { Bytes.empty() }
export function encodeUtf8(s: string) -> Bytes { Bytes.encodeUtf8(s) }
export function fromArray(values: Array<int>) -> Result<Bytes, string> { Bytes.fromArray(values) }
export function fromHex(s: string) -> Result<Bytes, string> { Bytes.fromHex(s) }
export function fromBase64(s: string) -> Result<Bytes, string> { Bytes.fromBase64(s) }
export function concat(a: Bytes, b: Bytes) -> Bytes { Bytes.concat(a, b) }
export function decodeUtf8(value: Bytes) -> Result<string, string> { value.decodeUtf8() }
export function length(value: Bytes) -> int { value.length() }
export function isEmpty(value: Bytes) -> bool { value.isEmpty() }
export function get(value: Bytes, index: int) -> Option<int> { value.get(index) }
export function slice(value: Bytes, start: int, end: int) -> Bytes { value.slice(start, end) }
export function toArray(value: Bytes) -> Array<int> { value.toArray() }
export function toHex(value: Bytes) -> string { value.toHex() }
export function toBase64(value: Bytes) -> string { value.toBase64() }
export function allocateBuffer(length: int, value: int = 0) -> ByteBuffer { ByteBuffer.allocate(length, value) }
export function bufferFromBytes(value: Bytes) -> ByteBuffer { ByteBuffer.fromBytes(value) }
export function bufferLength(value: ByteBuffer) -> int { value.length() }
export function bufferGet(value: ByteBuffer, index: int) -> int { value.get(index) }
export function bufferToBytes(value: ByteBuffer) -> Bytes { value.toBytes() }
"#;

const STD_HASH: &str = r#"
export function sha256(data: Bytes) -> Bytes { Hash.sha256(data) }
export function sha256Text(text: string) -> string { Hash.sha256(Bytes.encodeUtf8(text)).toHex() }
export function sha512(data: Bytes) -> Bytes { Hash.sha512(data) }
export function sha512Text(text: string) -> string { Hash.sha512(Bytes.encodeUtf8(text)).toHex() }
export function hmacSha256(key: Bytes, message: Bytes) -> Bytes { Hash.hmacSha256(key, message) }
export function crc32(data: Bytes) -> int { Hash.crc32(data) }
"#;

const STD_CLI: &str = r#"
export function hasFlag(args: Array<string>, name: string) -> bool { Cli.hasFlag(args, name) }
export function option(args: Array<string>, name: string) -> Option<string> { Cli.option(args, name) }
export function options(args: Array<string>, name: string) -> Array<string> { Cli.options(args, name) }
export function positionals(args: Array<string>) -> Array<string> { Cli.positionals(args) }
"#;

const STD_PATH: &str = r#"
export type Path = Path
export function from(text: string) -> Result<Path, string> { Path.from(text) }
export function cwdRelative(text: string) -> Result<Path, string> { Path.cwdRelative(text) }
export function project(text: string) -> Result<Path, string> { Path.project(text) }
export function join(path: Path, child: string) -> Result<Path, string> { path.join(child) }
export function parent(path: Path) -> Option<Path> { path.parent() }
export function fileName(path: Path) -> Option<string> { path.fileName() }
export function extension(path: Path) -> Option<string> { path.extension() }
export function withExtension(path: Path, ext: string) -> Result<Path, string> { path.withExtension(ext) }
export function normalize(path: Path) -> Path { path.normalize() }
export function toString(path: Path) -> string { path.toString() }
"#;

const STD_FS: &str = r#"
export type Bytes = Bytes
export type Path = Path
export function readText(path: Path | string) -> Result<string, string> { FS.readText(path) }
export function writeText(path: Path | string, text: string) -> Result<(), string> { FS.writeText(path, text) }
export function readBytes(path: Path | string) -> Result<Bytes, string> { FS.readBytes(path) }
export function writeBytes(path: Path | string, bytes: Bytes) -> Result<(), string> { FS.writeBytes(path, bytes) }
export function list(path: Path | string) -> Result<Array<{ name: string, kind: string, sizeBytes: Option<int> }>, string> { FS.list(path) }
"#;

const STD_IO: &str = r#"
export function readStdin() -> string { input() }
export function writeLine(line: string) -> () { print(line) }
"#;

const STD_REGEX: &str = r#"
export type Regex = Regex
export type Match = Match
export function compile(pattern: string) -> Result<Regex, string> { Regex.compile(pattern) }
export function isMatch(re: Regex, text: string) -> bool { re.isMatch(text) }
export function find(re: Regex, text: string) -> Option<Match> { re.find(text) }
export function findAll(re: Regex, text: string) -> Array<Match> { re.findAll(text) }
export function split(re: Regex, text: string) -> Array<string> { re.split(text) }
export function replaceAll(re: Regex, text: string, replacement: string) -> string { re.replaceAll(text, replacement) }
export function start(m: Match) -> int { m.start }
export function end(m: Match) -> int { m.end }
export function textOf(m: Match) -> string { m.text }
export function groups(m: Match) -> Array<Option<string>> { m.groups }
export function named(m: Match) -> Map<string, string> { m.named }
"#;

const STD_CSV: &str = r#"
export function parse(text: string) -> Result<Array<Array<string>>, string> { CSV.parse(text) }
export function parseWithHeader(text: string) -> Result<Array<Map<string, string>>, string> { CSV.parseWithHeader(text) }
export function stringify(rows: Array<Array<string>>) -> string { CSV.stringify(rows) }
export function stringifyWithHeader(rows: Array<Map<string, string>>, columns: Array<string>) -> string { CSV.stringifyWithHeader(rows, columns) }
"#;

const STD_TOML: &str = r#"
export type TOMLValue = TOMLValue
export function parse(text: string) -> Result<TOMLValue, string> { TOML.parse(text) }
export function stringify(value: TOMLValue) -> Result<string, string> { TOML.stringify(value) }
export function toJson(value: TOMLValue) -> JSONValue { TOML.toJson(value) }
export function fromJson(value: JSONValue) -> Result<TOMLValue, string> { TOML.fromJson(value) }
"#;

const STD_JSON: &str = r#"
export type JSONValue = JSONValue
export function parse(text: string) -> Result<JSONValue, { message: string, line: int, column: int }> { JSON.parse(text) }
export function stringify(value: JSONValue) -> Result<string, string> { JSON.stringify(value) }
export function kind(value: JSONValue) -> string { value.kind() }
export function isNull(value: JSONValue) -> bool { value.isNull() }
export function asString(value: JSONValue) -> Option<string> { value.asString() }
export function asBool(value: JSONValue) -> Option<bool> { value.asBool() }
export function asInt(value: JSONValue) -> Option<int> { value.asInt() }
export function numberText(value: JSONValue) -> Option<string> { value.numberText() }
export function get(value: JSONValue, key: string) -> Option<JSONValue> { value.get(key) }
export function at(value: JSONValue, index: int) -> Option<JSONValue> { value.at(index) }
export function length(value: JSONValue) -> Option<int> { value.length() }
export function asArray(value: JSONValue) -> Option<Array<JSONValue>> { value.asArray() }
export function keys(value: JSONValue) -> Option<Array<string>> { value.keys() }
export function values(value: JSONValue) -> Option<Array<JSONValue>> { value.values() }
"#;

const STD_URL: &str = r#"
export type URL = URL
export function parse(text: string) -> Result<URL, string> { URL.parse(text) }
export function scheme(url: URL) -> string { url.scheme() }
export function host(url: URL) -> Option<string> { url.host() }
export function path(url: URL) -> string { url.path() }
export function query(url: URL) -> Map<string, Array<string>> { url.query() }
export function fragment(url: URL) -> Option<string> { url.fragment() }
export function toString(url: URL) -> string { url.toString() }
"#;

const STD_HTTP: &str = r#"
export type Bytes = Bytes
export type URL = URL

export record HttpRequest derives Show {
  method: string,
  url: URL,
  headers: Map<string, Array<string>>,
  body: Bytes,
}

export record HttpResponse derives Show {
  status: int,
  headers: Map<string, Array<string>>,
  body: Bytes,
}

export function request(method: string, url: URL, headers: Map<string, Array<string>>, body: Bytes) -> HttpRequest {
  HttpRequest { method: method, url: url, headers: headers, body: body }
}

export function response(status: int, headers: Map<string, Array<string>>, body: Bytes) -> HttpResponse {
  HttpResponse { status: status, headers: headers, body: body }
}

export function text(status: int, body: string) -> HttpResponse {
  HttpResponse {
    status: status,
    headers: map { "content-type": ["text/plain; charset=utf-8"] },
    body: Bytes.encodeUtf8(body),
  }
}

export function json<T: JSON>(status: int, value: T) -> Result<HttpResponse, string> {
  let body = JSON.stringify(value)?
  Ok(HttpResponse {
    status: status,
    headers: map { "content-type": ["application/json"] },
    body: Bytes.encodeUtf8(body),
  })
}

export function header(req: HttpRequest, name: string) -> Option<string> {
  req.headers.get(name).flatMap((values) => values.get(0))
}
"#;

const STD_DOM: &str = r#"
export type JSONValue = JSONValue

export enum Html<Msg> {
  Text(string),
  Element(Element<Msg>),
}

export record Attr derives Show {
  name: string,
  value: string,
}

export record Event<Msg> {
  name: string,
  message: Msg,
}

export record BrowserEvent {
  kind: string,
  targetId: Option<string>,
  value: Option<string>,
  checked: Option<bool>,
  key: Option<string>,
}

export record Element<Msg> {
  tag: string,
  attrs: Array<Attr>,
  events: Array<Event<Msg>>,
  children: Array<Html<Msg>>,
}

export enum Command<Msg> {
  SetText(string, string),
  SetAttr(string, string, string),
  RemoveAttr(string, string),
  AddClass(string, string),
  RemoveClass(string, string),
  Focus(string),
  Navigate(string),
  Dispatch(Msg),
}

export record TextDocument {
  name: string,
  mediaType: string,
  sizeBytes: int,
  text: string,
}

export enum LocalDataResult {
  TextOpened(TextDocument),
  DownloadStarted(string),
  Cancelled,
  Failed(string, string),
}

export record LocalDataEvent {
  requestId: string,
  result: LocalDataResult,
}

export enum LocalStateResult {
  Loaded(string, Option<string>),
  Saved(string),
  Deleted(string, bool),
  Failed(string, string),
}

export record LocalStateEvent {
  requestId: string,
  result: LocalStateResult,
}

export enum WebAppEvent {
  Browser(BrowserEvent),
  LocalData(LocalDataEvent),
  LocalState(LocalStateEvent),
}

export enum WebAppCommand<Msg> {
  Dom(Command<Msg>),
  OpenText(string, string, Msg),
  DownloadText(string, string, string, string, Msg),
  LoadState(string, string, Msg),
  SaveState(string, string, string, Msg),
  DeleteState(string, string, Msg),
}

export record AppStep<Model, Msg> {
  model: Model,
  commands: Array<Command<Msg>>,
}

export record WebAppStep<Model, Msg> {
  model: Model,
  commands: Array<WebAppCommand<Msg>>,
}

export type TraceHtml = Html<JSONValue>
export type TraceEvent = Event<JSONValue>
export type TraceElement = Element<JSONValue>
export type TraceCommand = Command<JSONValue>
export type TraceWebAppCommand = WebAppCommand<JSONValue>

export function text<M>(value: string) -> Html<M> { Html.Text(value) }

export function attr(name: string, value: string) -> Attr {
  Attr { name: name, value: value }
}

export function event<M>(name: string, message: M) -> Event<M> {
  Event { name: name, message: message }
}

export function element<M>(tag: string, attrs: Array<Attr>, events: Array<Event<M>>, children: Array<Html<M>>) -> Html<M> {
  Html.Element(Element { tag: tag, attrs: attrs, events: events, children: children })
}

export function setText<M>(selector: string, text: string) -> Command<M> {
  Command.SetText(selector, text)
}

export function setAttr<M>(selector: string, name: string, value: string) -> Command<M> {
  Command.SetAttr(selector, name, value)
}

export function removeAttr<M>(selector: string, name: string) -> Command<M> {
  Command.RemoveAttr(selector, name)
}

export function addClass<M>(selector: string, name: string) -> Command<M> {
  Command.AddClass(selector, name)
}

export function removeClass<M>(selector: string, name: string) -> Command<M> {
  Command.RemoveClass(selector, name)
}

export function focus<M>(selector: string) -> Command<M> {
  Command.Focus(selector)
}

export function navigate<M>(url: string) -> Command<M> {
  Command.Navigate(url)
}

export function dispatch<M>(message: M) -> Command<M> {
  Command.Dispatch(message)
}

export function dom<M>(command: Command<M>) -> WebAppCommand<M> {
  WebAppCommand.Dom(command)
}

export function openText<M>(requestId: string, accept: string, message: M) -> WebAppCommand<M> {
  WebAppCommand.OpenText(requestId, accept, message)
}

export function downloadText<M>(requestId: string, filename: string, mediaType: string, value: string, message: M) -> WebAppCommand<M> {
  WebAppCommand.DownloadText(requestId, filename, mediaType, value, message)
}

export function loadState<M>(requestId: string, key: string, message: M) -> WebAppCommand<M> {
  WebAppCommand.LoadState(requestId, key, message)
}

export function saveState<M>(requestId: string, key: string, value: string, message: M) -> WebAppCommand<M> {
  WebAppCommand.SaveState(requestId, key, value, message)
}

export function deleteState<M>(requestId: string, key: string, message: M) -> WebAppCommand<M> {
  WebAppCommand.DeleteState(requestId, key, message)
}
"#;

const STD_ENCODING: &str = r#"
export type Bytes = Bytes
export function utf8Encode(text: string) -> Bytes { Encoding.utf8Encode(text) }
export function utf8Decode(bytes: Bytes) -> Result<string, string> { Encoding.utf8Decode(bytes) }
export function hexEncode(bytes: Bytes) -> string { Encoding.hexEncode(bytes) }
export function hexDecode(text: string) -> Result<Bytes, string> { Encoding.hexDecode(text) }
export function base64Encode(bytes: Bytes) -> string { Encoding.base64Encode(bytes) }
export function base64Decode(text: string) -> Result<Bytes, string> { Encoding.base64Decode(text) }
"#;

const STD_CODEC: &str = r#"
export type Bytes = Bytes
export function gzipCompress(bytes: Bytes) -> Result<Bytes, string> { Codec.gzipCompress(bytes) }
export function gzipDecompress(bytes: Bytes) -> Result<Bytes, string> { Codec.gzipDecompress(bytes) }
export function deflateCompress(bytes: Bytes) -> Result<Bytes, string> { Codec.deflateCompress(bytes) }
export function deflateFixedCompress(bytes: Bytes) -> Result<Bytes, string> { Codec.deflateFixedCompress(bytes) }
export function zlibFixedCompress(bytes: Bytes) -> Result<Bytes, string> { Codec.zlibFixedCompress(bytes) }
export function reedSolomon255223Protect(bytes: Bytes) -> Result<Bytes, string> { Codec.reedSolomon255223Protect(bytes) }
export function deflateDecompress(bytes: Bytes) -> Result<Bytes, string> { Codec.deflateDecompress(bytes) }
export function zstdCompress(bytes: Bytes, level: int = 3) -> Result<Bytes, string> { Codec.zstdCompress(bytes, level) }
export function zstdDecompress(bytes: Bytes) -> Result<Bytes, string> { Codec.zstdDecompress(bytes) }
"#;

const STD_DATE: &str = r#"
export type Date = Date
export function fromYmd(year: int, month: int, day: int) -> Result<Date, string> { Date.fromYmd(year, month, day) }
export function parseIso(text: string) -> Result<Date, string> { Date.parseIso(text) }
export function toIso(date: Date) -> string { date.toIso() }
export function addDays(date: Date, days: int) -> Date { date.addDays(days) }
export function year(date: Date) -> int { date.year() }
export function month(date: Date) -> int { date.month() }
export function day(date: Date) -> int { date.day() }
"#;

const STD_BIGINT: &str = r#"
export type BigInt = BigInt
export function fromInt(n: int) -> BigInt { BigInt.fromInt(n) }
export function parse(text: string, radix: int) -> Option<BigInt> { BigInt.parse(text, radix) }
export function toString(value: BigInt, radix: int = 10) -> string { value.toString(radix) }
export function asInt(value: BigInt) -> Option<int> { value.toInt() }
export function div(a: BigInt, b: BigInt) -> Result<BigInt, string> { a.div(b) }
export function modulo(a: BigInt, b: BigInt) -> Result<BigInt, string> { a.mod(b) }
"#;

const STD_DECIMAL: &str = r#"
export type Decimal = Decimal
export type RoundingMode = RoundingMode
export function fromInt(n: int) -> Decimal { Decimal.fromInt(n) }
export function parse(text: string) -> Option<Decimal> { Decimal.parse(text) }
export function toString(value: Decimal) -> string { value.toString() }
export function scale(value: Decimal) -> int { value.scale() }
export function asInt(value: Decimal) -> Option<int> { value.toInt() }
export function round(value: Decimal, scale: int) -> Decimal { value.round(scale) }
export function roundWithMode(value: Decimal, scale: int, mode: RoundingMode) -> Decimal { value.round(scale, mode) }
export function div(a: Decimal, b: Decimal, scale: int) -> Result<Decimal, string> { a.div(b, scale) }
export function divWithMode(a: Decimal, b: Decimal, scale: int, mode: RoundingMode) -> Result<Decimal, string> { a.div(b, scale, mode) }
"#;

const STD_GEN: &str = r#"
export function intRange(lo: int, hi: int) -> Array<int> {
  let mut values: Array<int> = []
  if hi < lo { return values }
  let mut x = lo
  while true {
    values.push(x)
    if x == hi { break }
    x += 1
  }
  return values
}

export function bools() -> Array<bool> {
  return [false, true]
}
"#;

const STD_PARSER: &str = r#"
export function isAsciiDigitCode(cp: int) -> bool {
  cp >= 48 && cp <= 57
}

export function isAsciiAlphaCode(cp: int) -> bool {
  (cp >= 65 && cp <= 90) || (cp >= 97 && cp <= 122)
}

export function isAsciiAlnumCode(cp: int) -> bool {
  isAsciiAlphaCode(cp) || isAsciiDigitCode(cp)
}

export function isAsciiWhitespaceCode(cp: int) -> bool {
  cp == 9 || cp == 10 || cp == 11 || cp == 12 || cp == 13 || cp == 32
}

export function isAsciiDigit(text: string, index: int) -> bool {
  match text.codePointAt(index) {
    case Some(cp) => isAsciiDigitCode(cp)
    case None => false
  }
}

export function isAsciiAlpha(text: string, index: int) -> bool {
  match text.codePointAt(index) {
    case Some(cp) => isAsciiAlphaCode(cp)
    case None => false
  }
}

export function isAsciiAlnum(text: string, index: int) -> bool {
  match text.codePointAt(index) {
    case Some(cp) => isAsciiAlnumCode(cp)
    case None => false
  }
}

export function isAsciiWhitespace(text: string, index: int) -> bool {
  match text.codePointAt(index) {
    case Some(cp) => isAsciiWhitespaceCode(cp)
    case None => false
  }
}

function clampStart(text: string, start: int) -> int {
  let limit = text.scalars().length
  if start < 0 { return 0 }
  if start > limit { return limit }
  return start
}

export function takeWhileAsciiDigit(text: string, start: int) -> int {
  let limit = text.scalars().length
  let mut i = clampStart(text, start)
  while i < limit && isAsciiDigit(text, i) {
    i += 1
  }
  return i
}

export function takeWhileAsciiAlpha(text: string, start: int) -> int {
  let limit = text.scalars().length
  let mut i = clampStart(text, start)
  while i < limit && isAsciiAlpha(text, i) {
    i += 1
  }
  return i
}

export function takeWhileAsciiAlnum(text: string, start: int) -> int {
  let limit = text.scalars().length
  let mut i = clampStart(text, start)
  while i < limit && isAsciiAlnum(text, i) {
    i += 1
  }
  return i
}

export function takeWhileAsciiWhitespace(text: string, start: int) -> int {
  let limit = text.scalars().length
  let mut i = clampStart(text, start)
  while i < limit && isAsciiWhitespace(text, i) {
    i += 1
  }
  return i
}
"#;

const STD_TEST: &str = r#"
export function assert(condition: bool, message: string = "assertion failed") -> () { Test.assert(condition, message) }
export function assertEq<T>(actual: T, expected: T) -> () { Test.assertEq(actual, expected) }
export function assertNe<T>(actual: T, expected: T) -> () { Test.assertNe(actual, expected) }
export function assertContains(text: string, needle: string) -> () { Test.assertContains(text, needle) }
export function assertOk<T, E>(value: Result<T, E>) -> T { Test.assertOk(value) }
export function assertErr<T, E>(value: Result<T, E>) -> E { Test.assertErr(value) }
export function assertSome<T>(value: Option<T>) -> T { Test.assertSome(value) }
export function assertNone<T>(value: Option<T>) -> () { Test.assertNone(value) }
export function assertGolden(path: string, actual: string) -> () { Test.assertGolden(path, actual) }
export function forAllInt(name: string, values: Array<int>, f: (int) -> ()) -> () {
  for value in values {
    f(value)
  }
  return ()
}
export function forAllBool(name: string, values: Array<bool>, f: (bool) -> ()) -> () {
  for value in values {
    f(value)
  }
  return ()
}
"#;
