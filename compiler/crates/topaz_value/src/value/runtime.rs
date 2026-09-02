use super::*;

/// Execute every pure unbound builtin whose complete semantics live in
/// `topaz_value`. The interpreter and generated runtime call this before their
/// engine-owned dispatch. A host-backed or engine-owned builtin leaves `args`
/// untouched so the caller can continue without cloning the argument vector.
pub fn call_pure_builtin(
    kind: Builtin,
    args: &mut Vec<Value>,
    span: Span,
) -> Option<Result<Value, RtError>> {
    macro_rules! execute {
        ($body:block) => {
            Some((|| -> Result<Value, RtError> { $body })())
        };
    }

    match kind {
        Builtin::ToInt => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_to_int(value, span)
        }),
        Builtin::ToIntRadix => execute!({
            let [text, radix] = exact_args(std::mem::take(args), span)?;
            builtin_to_int_radix(text, radix, span)
        }),
        Builtin::FromCodePoint => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_from_code_point(value, span)
        }),
        Builtin::ToFloat => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_to_float(value, span)
        }),
        Builtin::ArrayOf => Some(Ok(Value::array(std::mem::take(args)))),
        Builtin::MapNew => execute!({
            let [] = exact_args(std::mem::take(args), span)?;
            Ok(builtin_map_new())
        }),
        Builtin::MapOfEntries => execute!({
            let [entries] = exact_args(std::mem::take(args), span)?;
            builtin_map_of_entries(entries, span)
        }),
        Builtin::SetOf => Some(builtin_set_of(std::mem::take(args), span)),
        Builtin::JsonStringify => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            Ok(builtin_json_stringify(value))
        }),
        Builtin::JsonParse => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_json_parse(value, span)
        }),
        Builtin::MathSqrt => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_math_sqrt(value, span)
        }),
        Builtin::MathAbs => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_math_abs(value, span)
        }),
        Builtin::MathFloor => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_math_floor(value, span)
        }),
        Builtin::MathCeil => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_math_ceil(value, span)
        }),
        Builtin::MathRound => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_math_round(value, span)
        }),
        Builtin::MathSin => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_math_sin(value, span)
        }),
        Builtin::MathCos => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_math_cos(value, span)
        }),
        Builtin::MathTan => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_math_tan(value, span)
        }),
        Builtin::MathMin => execute!({
            let [a, b] = exact_args(std::mem::take(args), span)?;
            builtin_math_min(a, b, span)
        }),
        Builtin::MathMax => execute!({
            let [a, b] = exact_args(std::mem::take(args), span)?;
            builtin_math_max(a, b, span)
        }),
        Builtin::MathIsNaN => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_math_is_nan(value, span)
        }),
        Builtin::MathIsFinite => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_math_is_finite(value, span)
        }),
        Builtin::MathParseFloat => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_math_parse_float(value, span)
        }),
        Builtin::BytesEmpty => execute!({
            let [] = exact_args(std::mem::take(args), span)?;
            builtin_bytes_empty(span)
        }),
        Builtin::BytesEncodeUtf8 | Builtin::EncodingUtf8Encode => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_bytes_encode_utf8(value, span)
        }),
        Builtin::BytesFromArray => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_bytes_from_array(value, span)
        }),
        Builtin::BytesFromHex | Builtin::EncodingHexDecode => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_bytes_from_hex(value, span)
        }),
        Builtin::BytesFromBase64 | Builtin::EncodingBase64Decode => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_bytes_from_base64(value, span)
        }),
        Builtin::BytesConcat => execute!({
            let [a, b] = exact_args(std::mem::take(args), span)?;
            builtin_bytes_concat(a, b, span)
        }),
        Builtin::ByteBufferAllocate => execute!({
            let args = std::mem::take(args);
            let (size, value) = match args.len() {
                1 => {
                    let [size] = exact_args(args, span)?;
                    (size, None)
                }
                2 => {
                    let [size, value] = exact_args(args, span)?;
                    (size, Some(value))
                }
                found => {
                    return Err(fault(
                        codes::GUARD_ARITY,
                        format!("expected 1..2 argument(s), found {found}"),
                        span,
                    ));
                }
            };
            builtin_byte_buffer_allocate(size, value, span)
        }),
        Builtin::ByteBufferFromBytes => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_byte_buffer_from_bytes(value, span)
        }),
        Builtin::EncodingUtf8Decode => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_bytes_decode_utf8(value, span)
        }),
        Builtin::EncodingHexEncode => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_bytes_to_hex(value, span)
        }),
        Builtin::EncodingBase64Encode => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_bytes_to_base64(value, span)
        }),
        Builtin::CodecGzipCompress => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_codec_gzip_compress(value, span)
        }),
        Builtin::CodecGzipDecompress => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_codec_gzip_decompress(value, span)
        }),
        Builtin::CodecDeflateCompress => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_codec_deflate_compress(value, span)
        }),
        Builtin::CodecDeflateFixedCompress => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_codec_deflate_fixed_compress(value, span)
        }),
        Builtin::CodecZlibFixedCompress => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_codec_zlib_fixed_compress(value, span)
        }),
        Builtin::CodecReedSolomon255223Protect => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_codec_reed_solomon_255_223_protect(value, span)
        }),
        Builtin::CodecDeflateDecompress => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_codec_deflate_decompress(value, span)
        }),
        Builtin::CodecZstdCompress => execute!({
            let args = std::mem::take(args);
            let (bytes, level) = match args.len() {
                1 => {
                    let [bytes] = exact_args(args, span)?;
                    let level =
                        builtin_default_arg(Builtin::CodecZstdCompress, 1).ok_or_else(|| {
                            fault(
                                codes::GUARD_UNIMPLEMENTED,
                                "Codec.zstdCompress default level is unavailable",
                                span,
                            )
                        })?;
                    (bytes, level)
                }
                2 => {
                    let [bytes, level] = exact_args(args, span)?;
                    (bytes, level)
                }
                found => {
                    return Err(fault(
                        codes::GUARD_ARITY,
                        format!("expected 1..2 argument(s), found {found}"),
                        span,
                    ));
                }
            };
            builtin_codec_zstd_compress(bytes, level, span)
        }),
        Builtin::CodecZstdDecompress => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_codec_zstd_decompress(value, span)
        }),
        Builtin::HashSha256 => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_hash_sha256(value, span)
        }),
        Builtin::HashSha512 => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_hash_sha512(value, span)
        }),
        Builtin::HashHmacSha256 => execute!({
            let [key, message] = exact_args(std::mem::take(args), span)?;
            builtin_hash_hmac_sha256(key, message, span)
        }),
        Builtin::HashCrc32 => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_hash_crc32(value, span)
        }),
        Builtin::CliHasFlag => execute!({
            let [arguments, name] = exact_args(std::mem::take(args), span)?;
            builtin_cli_has_flag(arguments, name, span)
        }),
        Builtin::CliOption => execute!({
            let [arguments, name] = exact_args(std::mem::take(args), span)?;
            builtin_cli_option(arguments, name, span)
        }),
        Builtin::CliOptions => execute!({
            let [arguments, name] = exact_args(std::mem::take(args), span)?;
            builtin_cli_options(arguments, name, span)
        }),
        Builtin::CliPositionals => execute!({
            let [arguments] = exact_args(std::mem::take(args), span)?;
            builtin_cli_positionals(arguments, span)
        }),
        Builtin::PathFrom => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_path_from(value, span)
        }),
        Builtin::PathCwdRelative => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_path_cwd_relative(value, span)
        }),
        Builtin::PathProject => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_path_project(value, span)
        }),
        Builtin::RegexCompile => execute!({
            let [pattern] = exact_args(std::mem::take(args), span)?;
            builtin_regex_compile(pattern, span)
        }),
        Builtin::CsvParse => execute!({
            let [text] = exact_args(std::mem::take(args), span)?;
            builtin_csv_parse(text, span)
        }),
        Builtin::CsvParseWithHeader => execute!({
            let [text] = exact_args(std::mem::take(args), span)?;
            builtin_csv_parse_with_header(text, span)
        }),
        Builtin::CsvStringify => execute!({
            let [rows] = exact_args(std::mem::take(args), span)?;
            builtin_csv_stringify(rows, span)
        }),
        Builtin::CsvStringifyWithHeader => execute!({
            let [rows, columns] = exact_args(std::mem::take(args), span)?;
            builtin_csv_stringify_with_header(rows, columns, span)
        }),
        Builtin::TomlParse => execute!({
            let [text] = exact_args(std::mem::take(args), span)?;
            builtin_toml_parse(text, span)
        }),
        Builtin::TomlStringify => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_toml_stringify(value, span)
        }),
        Builtin::TomlToJson => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_toml_to_json(value, span)
        }),
        Builtin::TomlFromJson => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_toml_from_json(value, span)
        }),
        Builtin::UrlParse => execute!({
            let [text] = exact_args(std::mem::take(args), span)?;
            builtin_url_parse(text, span)
        }),
        Builtin::DateFromYmd => execute!({
            let [year, month, day] = exact_args(std::mem::take(args), span)?;
            builtin_date_from_ymd(year, month, day, span)
        }),
        Builtin::DateParseIso => execute!({
            let [text] = exact_args(std::mem::take(args), span)?;
            builtin_date_parse_iso(text, span)
        }),
        Builtin::BigIntFromInt => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_bigint_from_int(value, span)
        }),
        Builtin::BigIntParse => execute!({
            let [text, radix] = exact_args(std::mem::take(args), span)?;
            builtin_bigint_parse(text, radix, span)
        }),
        Builtin::DecimalFromInt => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_decimal_from_int(value, span)
        }),
        Builtin::DecimalParse => execute!({
            let [text] = exact_args(std::mem::take(args), span)?;
            builtin_decimal_parse(text, span)
        }),
        _ => None,
    }
}

/// Execute unbound builtins whose effects are completely expressed by the
/// shared host boundary. Engine-owned callbacks and continuations remain with
/// their evaluator; an unmatched builtin leaves `args` untouched.
pub fn call_host_builtin(
    host: &dyn Host,
    kind: Builtin,
    args: &mut Vec<Value>,
    span: Span,
) -> Option<Result<Value, RtError>> {
    macro_rules! execute {
        ($body:block) => {
            Some((|| -> Result<Value, RtError> { $body })())
        };
    }

    if let Some(operation) = kind.lispex_application_operation() {
        return Some(builtin_lispex_application(
            host,
            operation,
            std::mem::take(args),
            span,
        ));
    }

    match kind {
        Builtin::Print => execute!({
            let [value] = exact_args(std::mem::take(args), span)?;
            builtin_print(host, value, span)
        }),
        Builtin::Input => execute!({
            let [] = exact_args(std::mem::take(args), span)?;
            Ok(builtin_input(host))
        }),
        test @ (Builtin::TestAssert
        | Builtin::TestAssertEq
        | Builtin::TestAssertNe
        | Builtin::TestAssertContains
        | Builtin::TestAssertOk
        | Builtin::TestAssertErr
        | Builtin::TestAssertSome
        | Builtin::TestAssertNone
        | Builtin::TestAssertGolden) => Some(builtin_test_dispatch(
            host,
            test,
            std::mem::take(args),
            span,
        )),
        Builtin::Open => execute!({
            let [path] = exact_args(std::mem::take(args), span)?;
            match path {
                Value::Str(path) => match host.open(&path) {
                    Ok(handle) => Ok(Value::Ok(Rc::new(Value::Resource(handle)))),
                    Err(message) => Ok(Value::Err(Rc::new(Value::str(message)))),
                },
                other => Err(fault(
                    codes::GUARD_TYPE,
                    format!("`open` takes a `string`, found `{}`", other.kind()),
                    span,
                )),
            }
        }),
        Builtin::FsReadText => execute!({
            let [path] = exact_args(std::mem::take(args), span)?;
            builtin_fs_read_text(host, path, span)
        }),
        Builtin::FsWriteText => execute!({
            let [path, text] = exact_args(std::mem::take(args), span)?;
            builtin_fs_write_text(host, path, text, span)
        }),
        Builtin::FsReadBytes => execute!({
            let [path] = exact_args(std::mem::take(args), span)?;
            builtin_fs_read_bytes(host, path, span)
        }),
        Builtin::FsWriteBytes => execute!({
            let [path, bytes] = exact_args(std::mem::take(args), span)?;
            builtin_fs_write_bytes(host, path, bytes, span)
        }),
        Builtin::FsList => execute!({
            let [path] = exact_args(std::mem::take(args), span)?;
            builtin_fs_list(host, path, span)
        }),
        _ => None,
    }
}

/// The effect boundary (CDR-003 §1): the evaluator core never touches
/// `std::fs`/`std::io`/clocks/threads — every observable effect
/// crosses this trait, which keeps the core WASM-compatible and the
/// execution corpus deterministic. Declared here (the bottom ABI
/// layer); the implementations (`NativeHost`, capturing test hosts)
/// live at the leaves.
pub trait Host {
    /// §22.2 `print`: one line, newline appended by the host.
    fn print(&self, line: &str);
    /// §22.3 `open`.
    fn open(&self, path: &str) -> Result<ResourceId, String>;
    /// §22.3 `file.read()`.
    fn read(&self, handle: ResourceId) -> Result<String, String>;
    /// §22.3 `file.write(s)`.
    fn write(&self, handle: ResourceId, s: &str) -> Result<(), String>;
    /// §22.3 `file.close()`.
    fn close(&self, handle: ResourceId);
    /// Monotonic milliseconds for §15 timeout deadlines.
    fn now_millis(&self) -> u64;
    /// §14 runtime policy: an error escaping a deferred action is
    /// reported here and never replaces an in-flight result.
    fn defer_error(&self, rendered: &str);
    /// §22 `input()`: the host-provided per-run text payload (the WASM shell's
    /// textarea value, a native binary's piped stdin, or `""` when none).
    /// DETERMINISTIC — the same string on every call within one run, so both
    /// engines and repeated calls agree. A host with no input returns `""`.
    fn input(&self) -> String {
        String::new()
    }
    /// §10 `FS.readBytes(path)`.
    fn read_bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        let handle = self.open(path)?;
        let text = self.read(handle);
        self.close(handle);
        text.map(|s| s.into_bytes())
    }
    /// §10 `FS.writeBytes(path, bytes)`.
    fn write_bytes(&self, _path: &str, _bytes: &[u8]) -> Result<(), String> {
        Err("FS.writeBytes is not supported by this host".to_string())
    }
    /// §10 `FS.list(path)`, sorted by `name` before returning.
    fn list_dir(&self, _path: &str) -> Result<Vec<HostDirEntry>, String> {
        Err("FS.list is not supported by this host".to_string())
    }
    /// v5.4 manifest extern replay sandbox boundary.
    ///
    /// Hosts must route extern calls through the deterministic replay sandbox
    /// leaf. `kind = "wasm"` and artifact paths are admission/lock metadata in
    /// v5.4; they do not grant live artifact execution or ambient host effects.
    fn extern_call(&self, module: &str, function: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!(
            "extern replay for `{module}.{function}` is not available on this host"
        ))
    }
    /// Dedicated first-class bounded Lispex application capability.
    ///
    /// This is deliberately separate from extern replay and defaults to deny.
    /// Only a package host built from an exact checked rule closure may admit
    /// requests.  A host must never discover a component, callback into Topaz,
    /// or silently fall back from this boundary.
    fn lispex_application(
        &self,
        _request: crate::lispex_application::LispexApplicationRequest,
    ) -> crate::lispex_application::LispexApplicationResponse;
}

/// The cloneable, `Rc`-backed, `'static` run context handle
/// (CDR-006 §3): owns the host and call-depth state for the run's duration.
/// It is the opaque handle `TpzCall::call` receives.
/// Interior mutability only — never a `&mut` borrow — so §5 can keep
/// many suspended arm futures alive at once.
#[derive(Clone)]
pub struct RtCx(Rc<RtCxInner>);

pub(super) struct RtCxInner {
    host: Rc<dyn Host>,
    module_stable_nominals: bool,
    /// §4 the live count of nested Topaz CALLS (closure applications). Incremented on
    /// entry, decremented on exit by [`CallDepthGuard`]; `call_value` faults
    /// `GUARD_RECURSION` once it would exceed [`CALL_DEPTH_LIMIT`], so the emitted
    /// native recursion cannot overflow the stack where the interpreter would not.
    call_depth: Cell<usize>,
}

impl RtCx {
    pub fn new(host: Rc<dyn Host>) -> Self {
        RtCx(Rc::new(RtCxInner {
            host,
            module_stable_nominals: false,
            call_depth: Cell::new(0),
        }))
    }

    pub fn new_module_stable(host: Rc<dyn Host>) -> Self {
        RtCx(Rc::new(RtCxInner {
            host,
            module_stable_nominals: true,
            call_depth: Cell::new(0),
        }))
    }

    /// The run's host (host effects cross here).
    pub fn host(&self) -> Rc<dyn Host> {
        self.0.host.clone()
    }

    pub fn module_stable_nominals(&self) -> bool {
        self.0.module_stable_nominals
    }

    /// §4 the current nested-call depth (read by `call_value`'s recursion guard).
    pub fn call_depth(&self) -> usize {
        self.0.call_depth.get()
    }

    /// §4 enter one nested call: bump the depth and hand back a guard that restores it
    /// on drop (so the count is correct on every exit path — normal, `?`, or panic).
    pub fn enter_call(&self) -> CallDepthGuard {
        self.0.call_depth.set(self.0.call_depth.get() + 1);
        CallDepthGuard(self.clone())
    }

    /// §4/§7 directly set the nested-call depth — used ONLY by the concurrent executor's
    /// per-arm depth scoping (each `concurrent` arm runs with its OWN isolated counter,
    /// so interleaved/abandoned arms neither accumulate nor leak into the shared count).
    pub fn set_call_depth(&self, depth: usize) {
        self.0.call_depth.set(depth);
    }
}

/// RAII restore for [`RtCx::enter_call`] — decrements the nested-call depth when the
/// emitted call future completes, whatever the exit path.
pub struct CallDepthGuard(RtCx);

impl Drop for CallDepthGuard {
    fn drop(&mut self) {
        let d = self.0.0.call_depth.get();
        self.0.0.call_depth.set(d.saturating_sub(1));
    }
}

/// A future of a runtime result — the object-safe, non-`Send`,
/// `'static`, non-borrowed shape `TpzCall::call` returns so §5 can
/// hold many suspended calls at once (CDR-006 §3/§4).
pub type CallFuture = Pin<Box<dyn Future<Output = Result<Value, RtError>>>>;

/// A callable value's behavior (CDR-006 §3). The interpreter's
/// AST-backed closures and emitted compiled functions both implement
/// it; the interpreter recovers its own closure by downcasting
/// through [`TpzCall::as_any`] rather than calling [`TpzCall::call`].
pub trait TpzCall: std::fmt::Debug {
    /// Downcast seam: the interpreter recovers its concrete closure
    /// to frame-execute it.
    fn as_any(&self) -> &dyn Any;
    /// Diagnostic name for `<function NAME>` rendering (§2).
    fn name(&self) -> Option<&str>;
    /// The emitted-code call ABI (CDR-006 §4). The interpreter implements it
    /// but reaches its closures by downcast instead.
    fn call(&self, cx: RtCx, args: Vec<Value>) -> CallFuture;
    /// §5 fixed parameter count — the emitted call site (`call_value`)
    /// uses it to raise the arity fault the interpreter's `apply_call`
    /// would, since `call` itself has no span.
    fn arity(&self) -> usize;
    /// The `n`th parameter's name (for the §5 "missing argument for
    /// parameter `<name>`" fault), or `None` past the last.
    fn param_name(&self, n: usize) -> Option<&str>;
    /// Whether the `n`th parameter declares a §7 default. This is separate from
    /// [`Self::param_default`] so shape checks can ask about optionality without
    /// evaluating a default expression.
    fn has_param_default(&self, _n: usize) -> bool {
        false
    }
    /// Evaluate the `n`th parameter's §7 default at call time, under the callable's
    /// defining environment. Scalar literals may return an immediately-ready future;
    /// captured identifiers use a closure thunk so emitted call binding mirrors the
    /// interpreter's lazy `const_eval` default slot fill.
    fn param_default(&self, _n: usize, _cx: RtCx) -> Option<CallFuture> {
        None
    }
    /// §5 whether the callable has a trailing VARIADIC parameter: `arity()` is
    /// then the FIXED-parameter count, and the emitted `call_value` accepts any
    /// number of arguments at or above it (the surplus collects into the
    /// variadic's array INSIDE the call), instead of faulting "found more".
    /// `false` for every callable by default; only an emitted closure built from
    /// a `function` with a `...rest` parameter overrides it. (The interpreter
    /// reaches its closures by downcast, so its `ClosureData` impl is unaffected.)
    fn is_variadic(&self) -> bool {
        false
    }
}

/// A §16 tagged-template value's behavior. v0.3 templates are inert
/// (no execution; `sql` never text-inserts) — the trait only renders
/// the stable diagnostic form (CDR-003 §8).
pub trait TpzTemplate: std::fmt::Debug {
    fn as_any(&self) -> &dyn Any;
    /// Append the §2 reference rendering of this template.
    fn render_into(&self, out: &mut String);
}

/// §22.2 `print(s)` — the SHARED builtin both engines call (CDR-006 §2),
/// so its string-only guard and the host effect cannot drift. `print` is
/// string-only: a non-string faults with the interpolation hint (the
/// host appends the newline). Returns `Unit`. The host effect crosses
/// here, so emitted code and the interpreter write the SAME transcript.
pub fn builtin_print(host: &dyn Host, arg: Value, span: Span) -> Result<Value, RtError> {
    match arg {
        Value::Str(s) => {
            host.print(&s);
            Ok(Value::Unit)
        }
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`print` is string-only; interpolate `{}` instead (§22.2)",
                other.kind()
            ),
            span,
        )),
    }
}
