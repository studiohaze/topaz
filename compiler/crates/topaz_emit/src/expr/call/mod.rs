//! Rust emission for free, namespace, receiver, and optional calls.
//! Argument binding decisions arrive from checked lowering; this layer preserves
//! their dispatch shape while producing the boxed runtime calls.

use crate::*;

mod free;
mod namespace;
mod optional;
mod receiver;

pub(crate) use free::*;
pub(crate) use namespace::*;
pub(crate) use optional::*;
pub(crate) use receiver::*;

/// §22.2/§5: simulate the interpreter's builtin argument binding for the
/// strictly-unary `okOrElse` builtin (its sole parameter is `f`, per
/// `builtin_param_names`, then `apply_call` enforces arity/laziness). Returns the
/// GUARD_ARITY fault message the interpreter would raise for the given STATIC arg
/// shape on a REAL Option receiver, or `None` when the args bind cleanly to the
/// single callback slot (then the emit runs the lazy unary bridge). A record-field
/// shadow is handled by the dispatch's `Some(__field)` arm, not this simulation.
///
/// Mirrors `apply_call`'s builtin path: with NO named args the interpreter skips
/// binding and checks arity on the positional count directly; with named args it
/// fills positional into leading slots then binds each named by name (`f`), faulting
/// `parameter f is given twice` on a re-fill and `no parameter named <n>` on a name
/// that is not `f`, before the final arity check.
pub(crate) fn ok_or_else_bind_fault(
    positional_count: usize,
    named_names: &[&str],
) -> Option<String> {
    if named_names.is_empty() {
        return if positional_count == 1 {
            None
        } else {
            Some(format!("expected 1 argument(s), found {positional_count}"))
        };
    }
    // One slot, named `f`; positional args fill leading slots.
    let slots_len = positional_count.max(1);
    let mut filled = positional_count >= 1;
    for &n in named_names {
        if n == "f" {
            if filled {
                return Some("parameter `f` is given twice (§5)".to_string());
            }
            filled = true;
        } else {
            return Some(format!("no parameter named `{n}` (§5)"));
        }
    }
    if !filled {
        return Some("missing argument for parameter `f` (§5)".to_string());
    }
    if slots_len != 1 {
        return Some(format!("expected 1 argument(s), found {slots_len}"));
    }
    None
}

impl RenderedOkOrElseArgs<'_> {
    pub(crate) fn shadow_call(&self, callee: &str, call_span: &str) -> String {
        let positional = self
            .positional
            .iter()
            .map(|index| self.values[*index].as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if self.named.is_empty() {
            return format!(
                "call_value({callee}, vec![{positional}], cx.clone(), {call_span}).await?"
            );
        }
        let named = self
            .named
            .iter()
            .map(|(name, index)| {
                let value = &self.values[*index];
                format!("({name:?}.to_string(), {value})")
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "call_value_named({callee}, vec![{positional}], vec![{named}], cx.clone(), {call_span}).await?"
        )
    }

    pub(crate) fn builtin_arm(&self, member_span: &str, call_span: &str) -> String {
        let named_names = self.named.iter().map(|(name, _)| *name).collect::<Vec<_>>();
        emit_ok_or_else_builtin_arm(
            &self.values,
            self.positional.len(),
            &named_names,
            member_span,
            call_span,
        )
    }
}

pub(crate) fn render_ok_or_else_args<'ctx>(
    args: &[CallArg],
    mode: OkOrElseCallMode<'_>,
    ctx: ExprEmitContext<'ctx, '_, '_>,
) -> Result<RenderedOkOrElseArgs<'ctx>, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let (leading, reject_placeholders) = match mode {
        OkOrElseCallMode::Direct => (None, false),
        OkOrElseCallMode::Optional { leading } => (leading, true),
    };
    let mut values = Vec::with_capacity(args.len() + usize::from(leading.is_some()));
    let mut positional = Vec::new();
    let mut named = Vec::new();
    let mut saw_named = false;
    let mut consumed_args = false;

    if let Some(lead) = leading {
        let value_index = values.len();
        values.push(lead.to_string());
        if let [CallArg::Named { name, value }] = args
            && text(src, name.span) == "f"
            && matches!(&value.kind, ExprKind::Placeholder)
        {
            named.push((text(src, name.span), value_index));
            consumed_args = true;
        } else {
            positional.push(value_index);
        }
    }

    if !consumed_args {
        for arg in args {
            match arg {
                CallArg::Positional(_) if saw_named => {
                    return Err(EmitError::unsupported("call argument shape"));
                }
                CallArg::Positional(expr) => {
                    if reject_placeholders && contains_placeholder(expr) {
                        return Err(EmitError::unsupported("pipe placeholder"));
                    }
                    let value_index = values.len();
                    values.push(emit_expr(expr, src, aliases, locals, in_loop)?);
                    positional.push(value_index);
                }
                CallArg::Named { name, value } => {
                    saw_named = true;
                    if reject_placeholders && contains_placeholder(value) {
                        return Err(EmitError::unsupported("pipe placeholder"));
                    }
                    let value_index = values.len();
                    values.push(emit_expr(value, src, aliases, locals, in_loop)?);
                    named.push((text(src, name.span), value_index));
                }
                CallArg::Spread(_) => {
                    return Err(EmitError::unsupported("call argument shape"));
                }
            }
        }
    }

    Ok(RenderedOkOrElseArgs {
        values,
        positional,
        named,
    })
}

/// Emit the builtin half of an `okOrElse` member dispatch after the receiver's
/// record-field shadow has been excluded. One helper owns static builtin binding,
/// argument-effect retention on binding faults, and the generated runtime bridge.
pub(crate) fn emit_ok_or_else_builtin_arm(
    arg_rs: &[String],
    positional_count: usize,
    named_names: &[&str],
    member_span: &str,
    call_span: &str,
) -> String {
    match ok_or_else_bind_fault(positional_count, named_names) {
        None => format!(
            "check_member_method(&__recv, \"okOrElse\", {member_span})?; \
             call_callback_ok_or_else(__recv, {}, cx.clone(), {member_span}, {call_span}).await?",
            arg_rs[0]
        ),
        Some(message) => format!(
            "check_member_method(&__recv, \"okOrElse\", {member_span})?; \
             let _: Vec<Value> = vec![{}]; \
             return Err(fault(codes::GUARD_ARITY, {message:?}, {call_span}))",
            arg_rs.join(", ")
        ),
    }
}

/// §5: a call's arguments must be `positional* spread? named*` — named arguments
/// follow ALL positional/spread ones. A positional (or spread) that FOLLOWS a named
/// arg is a §5 violation the checker rejects (and the interpreter's `KCallArgs`
/// faults at runtime, AFTER evaluating the offending arg, with `positional
/// arguments may not follow named arguments (§5)`). The emitter collects positional
/// and named arguments SEPARATELY, which for this out-of-order shape would both
/// reorder the argument effects and mis-bind (a wrong fault) — so it cannot
/// faithfully lower it and honestly refuses (only `--unchecked` reaches the shape; a
/// checked build never does, so refusing breaks no valid program).
pub(crate) fn positional_after_named(args: &[CallArg]) -> bool {
    let mut saw_named = false;
    for a in args {
        match a {
            CallArg::Named { .. } => saw_named = true,
            CallArg::Positional(_) | CallArg::Spread(_) if saw_named => return true,
            _ => {}
        }
    }
    false
}

pub(crate) fn emit_call_arg_order_fault(
    args: &[CallArg],
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
    in_loop: bool,
    call_span: Span,
) -> Result<String, EmitError> {
    let mut saw_named = false;
    let mut saw_spread = false;
    let mut body = String::from("{ ");
    for arg in args {
        match arg {
            CallArg::Named { value, .. } => {
                saw_named = true;
                body.push_str(&format!(
                    "let _ = {}; ",
                    emit_expr(value, src, aliases, locals, in_loop)?
                ));
            }
            CallArg::Positional(expr) if saw_named => {
                body.push_str(&format!(
                    "let _ = {}; let __v: Value = return Err(fault(codes::GUARD_ARITY, {:?}, {})); __v }}",
                    emit_expr(expr, src, aliases, locals, in_loop)?,
                    "positional arguments may not follow named arguments (§5)",
                    emit_span(call_span)
                ));
                return Ok(body);
            }
            CallArg::Positional(expr) => {
                body.push_str(&format!(
                    "let _ = {}; ",
                    emit_expr(expr, src, aliases, locals, in_loop)?
                ));
            }
            CallArg::Spread(expr) if saw_named => {
                body.push_str(&format!(
                    "let _ = {}; let __v: Value = return Err(fault(codes::GUARD_ARITY, {:?}, {})); __v }}",
                    emit_expr(expr, src, aliases, locals, in_loop)?,
                    "named arguments must follow spread arguments (§5)",
                    emit_span(call_span)
                ));
                return Ok(body);
            }
            CallArg::Spread(expr) => {
                if !saw_spread {
                    body.push_str("let mut __tpz_order_spread: Vec<Value> = Vec::new(); ");
                    saw_spread = true;
                }
                body.push_str(&format!(
                    "call_spread_extend(&mut __tpz_order_spread, {}, {})?; ",
                    emit_expr(expr, src, aliases, locals, in_loop)?,
                    emit_span(expr.span)
                ));
            }
        }
    }
    Err(EmitError::unsupported("call argument shape").at(call_span))
}

/// §5/§8/§15 bind a fixed-arity namespace call's arguments to `params` in
/// parameter ORDER, accepting positional OR named (a named arg must name the
/// parameter it fills — the SAME names the checker/interpreter bind, so a
/// mis-named arg cannot silently lower under `--unchecked`). Returns the argument
/// expressions in parameter order. A wrong count / spread / unknown or duplicate
/// name is a structural `unsupported` (the checker already rejected such a call,
/// so this is the emit-side backstop). Mirrors the JSON arg-shape gate.
pub(crate) fn emit_fixed_namespace_call(
    args: &[CallArg],
    params: &[&str],
    defaults: &[Option<&str>],
    locate_spread_at_argument: bool,
    ctx: ExprEmitContext<'_, '_, '_>,
    call_span: Span,
    render_body: impl FnOnce(&[&str]) -> String,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    let default_count = defaults.iter().filter(|value| value.is_some()).count();
    if args.len() < params.len() - default_count || args.len() > params.len() {
        return Err(EmitError::unsupported("namespace call argument count"));
    }
    let mut source_slots = vec![None; params.len()];
    let mut ordered = Vec::with_capacity(args.len());
    let mut source_params = Vec::with_capacity(args.len());
    let mut next_positional = 0usize;
    let mut saw_named = false;
    for arg in args {
        match arg {
            CallArg::Positional(expr) if saw_named => {
                return emit_call_arg_order_fault(args, src, aliases, locals, in_loop, call_span);
            }
            CallArg::Positional(expr) => {
                if next_positional >= params.len() || source_slots[next_positional].is_some() {
                    return Err(EmitError::unsupported("namespace call argument shape"));
                }
                let rendered = emit_expr(expr, src, aliases, locals, in_loop)?;
                source_slots[next_positional] = Some(ordered.len());
                source_params.push(next_positional);
                ordered.push(rendered);
                next_positional += 1;
            }
            CallArg::Named { name, value } => {
                saw_named = true;
                let nm = text(src, name.span);
                let Some(idx) = params.iter().position(|p| *p == nm) else {
                    return Err(EmitError::unsupported("namespace call argument name"));
                };
                if source_slots[idx].is_some() {
                    return Err(EmitError::unsupported("namespace call duplicate argument"));
                }
                let rendered = emit_expr(value, src, aliases, locals, in_loop)?;
                source_slots[idx] = Some(ordered.len());
                source_params.push(idx);
                ordered.push(rendered);
            }
            CallArg::Spread(expr) => {
                let error = EmitError::unsupported("namespace call spread argument");
                return Err(if locate_spread_at_argument {
                    error.at(expr.span)
                } else {
                    error
                });
            }
        }
    }
    let already_canonical = source_params.windows(2).all(|pair| pair[0] < pair[1]);
    if ordered.len() <= 1 || already_canonical {
        let mut rendered_slots = Vec::with_capacity(params.len());
        for (param_index, source_index) in source_slots.iter().enumerate() {
            if let Some(source_index) = source_index {
                rendered_slots.push(ordered[*source_index].as_str());
            } else if let Some(default) = defaults.get(param_index).and_then(|value| *value) {
                rendered_slots.push(default);
            } else {
                return Err(EmitError::unsupported("namespace call missing argument"));
            }
        }
        return Ok(render_body(&rendered_slots));
    }
    let mut lets = String::new();
    let mut temp_names = Vec::with_capacity(ordered.len());
    for (source_index, rendered) in ordered.iter().enumerate() {
        let temp = format!("__tpz_ns_arg_{source_index}");
        lets.push_str(&format!("let {temp} = {rendered}; "));
        temp_names.push(temp);
    }
    let mut rendered_slots = Vec::with_capacity(params.len());
    for (param_index, source_index) in source_slots.iter().enumerate() {
        if let Some(source_index) = source_index {
            rendered_slots.push(temp_names[*source_index].as_str());
        } else if let Some(default) = defaults.get(param_index).and_then(|value| *value) {
            rendered_slots.push(default);
        } else {
            return Err(EmitError::unsupported("namespace call missing argument"));
        }
    }
    Ok(format!("{{ {lets}{} }}", render_body(&rendered_slots)))
}

pub(crate) fn render_fallible_namespace_call(leaf: &str, rendered: &[&str], span: &str) -> String {
    let call_args = if rendered.is_empty() {
        span.to_string()
    } else {
        format!("{}, {}", rendered.join(", "), span)
    };
    format!("{leaf}({call_args})?")
}

pub(crate) fn fixed_namespace_spec(namespace: &str, member: &str) -> Option<FixedNamespaceSpec> {
    use FixedNamespaceRuntime::{Host, Shared};

    let (leaf, params, runtime) = match (namespace, member) {
        ("Math", "sqrt") => ("builtin_math_sqrt", &["x"][..], Shared),
        ("Math", "abs") => ("builtin_math_abs", &["x"][..], Shared),
        ("Math", "floor") => ("builtin_math_floor", &["x"][..], Shared),
        ("Math", "ceil") => ("builtin_math_ceil", &["x"][..], Shared),
        ("Math", "round") => ("builtin_math_round", &["x"][..], Shared),
        ("Math", "sin") => ("builtin_math_sin", &["x"][..], Shared),
        ("Math", "cos") => ("builtin_math_cos", &["x"][..], Shared),
        ("Math", "tan") => ("builtin_math_tan", &["x"][..], Shared),
        ("Math", "isNaN") => ("builtin_math_is_nan", &["x"][..], Shared),
        ("Math", "isFinite") => ("builtin_math_is_finite", &["x"][..], Shared),
        ("Math", "parseFloat") => ("builtin_math_parse_float", &["s"][..], Shared),
        ("Math", "min") => ("builtin_math_min", &["a", "b"][..], Shared),
        ("Math", "max") => ("builtin_math_max", &["a", "b"][..], Shared),
        ("Bytes", "empty") => ("builtin_bytes_empty", &[][..], Shared),
        ("Bytes", "encodeUtf8") => ("builtin_bytes_encode_utf8", &["s"][..], Shared),
        ("Bytes", "fromArray") => ("builtin_bytes_from_array", &["values"][..], Shared),
        ("Bytes", "fromHex") => ("builtin_bytes_from_hex", &["s"][..], Shared),
        ("Bytes", "fromBase64") => ("builtin_bytes_from_base64", &["s"][..], Shared),
        ("Bytes", "concat") => ("builtin_bytes_concat", &["a", "b"][..], Shared),
        ("Encoding", "utf8Encode") => ("builtin_bytes_encode_utf8", &["text"][..], Shared),
        ("Encoding", "utf8Decode") => ("builtin_bytes_decode_utf8", &["bytes"][..], Shared),
        ("Encoding", "hexEncode") => ("builtin_bytes_to_hex", &["bytes"][..], Shared),
        ("Encoding", "hexDecode") => ("builtin_bytes_from_hex", &["text"][..], Shared),
        ("Encoding", "base64Encode") => ("builtin_bytes_to_base64", &["bytes"][..], Shared),
        ("Encoding", "base64Decode") => ("builtin_bytes_from_base64", &["text"][..], Shared),
        ("Codec", "gzipCompress") => ("builtin_codec_gzip_compress", &["bytes"][..], Shared),
        ("Codec", "gzipDecompress") => ("builtin_codec_gzip_decompress", &["bytes"][..], Shared),
        ("Codec", "deflateCompress") => ("builtin_codec_deflate_compress", &["bytes"][..], Shared),
        ("Codec", "deflateFixedCompress") => (
            "builtin_codec_deflate_fixed_compress",
            &["bytes"][..],
            Shared,
        ),
        ("Codec", "zlibFixedCompress") => {
            ("builtin_codec_zlib_fixed_compress", &["bytes"][..], Shared)
        }
        ("Codec", "reedSolomon255223Protect") => (
            "builtin_codec_reed_solomon_255_223_protect",
            &["bytes"][..],
            Shared,
        ),
        ("Codec", "deflateDecompress") => {
            ("builtin_codec_deflate_decompress", &["bytes"][..], Shared)
        }
        ("Codec", "zstdDecompress") => ("builtin_codec_zstd_decompress", &["bytes"][..], Shared),
        ("Codec", "zstdCompress") => (
            "builtin_codec_zstd_compress",
            &["bytes", "level"][..],
            Shared,
        ),
        ("Hash", "sha256") => ("builtin_hash_sha256", &["data"][..], Shared),
        ("Hash", "sha512") => ("builtin_hash_sha512", &["data"][..], Shared),
        ("Hash", "hmacSha256") => ("builtin_hash_hmac_sha256", &["key", "message"][..], Shared),
        ("Hash", "crc32") => ("builtin_hash_crc32", &["data"][..], Shared),
        ("FS", "readText") => ("builtin_fs_read_text", &["path"][..], Host),
        ("FS", "writeText") => ("builtin_fs_write_text", &["path", "text"][..], Host),
        ("FS", "readBytes") => ("builtin_fs_read_bytes", &["path"][..], Host),
        ("FS", "writeBytes") => ("builtin_fs_write_bytes", &["path", "bytes"][..], Host),
        ("FS", "list") => ("builtin_fs_list", &["path"][..], Host),
        ("Cli", "hasFlag") => ("builtin_cli_has_flag", &["args", "name"][..], Shared),
        ("Cli", "option") => ("builtin_cli_option", &["args", "name"][..], Shared),
        ("Cli", "options") => ("builtin_cli_options", &["args", "name"][..], Shared),
        ("Cli", "positionals") => ("builtin_cli_positionals", &["args"][..], Shared),
        ("Path", "from") => ("builtin_path_from", &["text"][..], Shared),
        ("Path", "cwdRelative") => ("builtin_path_cwd_relative", &["text"][..], Shared),
        ("Path", "project") => ("builtin_path_project", &["text"][..], Shared),
        ("Regex", "compile") => ("builtin_regex_compile", &["pattern"][..], Shared),
        ("CSV", "parse") => ("builtin_csv_parse", &["text"][..], Shared),
        ("CSV", "parseWithHeader") => ("builtin_csv_parse_with_header", &["text"][..], Shared),
        ("CSV", "stringify") => ("builtin_csv_stringify", &["rows"][..], Shared),
        ("CSV", "stringifyWithHeader") => (
            "builtin_csv_stringify_with_header",
            &["rows", "columns"][..],
            Shared,
        ),
        ("TOML", "parse") => ("builtin_toml_parse", &["text"][..], Shared),
        ("TOML", "stringify") => ("builtin_toml_stringify", &["value"][..], Shared),
        ("TOML", "toJson") => ("builtin_toml_to_json", &["value"][..], Shared),
        ("TOML", "fromJson") => ("builtin_toml_from_json", &["value"][..], Shared),
        ("URL", "parse") => ("builtin_url_parse", &["text"][..], Shared),
        ("Date", "fromYmd") => (
            "builtin_date_from_ymd",
            &["year", "month", "day"][..],
            Shared,
        ),
        ("Date", "parseIso") => ("builtin_date_parse_iso", &["text"][..], Shared),
        ("BigInt", "fromInt") => ("builtin_bigint_from_int", &["n"][..], Shared),
        ("BigInt", "parse") => ("builtin_bigint_parse", &["text", "radix"][..], Shared),
        ("Decimal", "fromInt") => ("builtin_decimal_from_int", &["n"][..], Shared),
        ("Decimal", "parse") => ("builtin_decimal_parse", &["text"][..], Shared),
        _ => return None,
    };

    let (defaults, locate_spread_at_argument) = match (namespace, member) {
        ("Codec", "zstdCompress") => (&[None, Some("Value::Int(3)")][..], false),
        _ => (&[][..], true),
    };
    Some(FixedNamespaceSpec {
        leaf,
        params,
        defaults,
        locate_spread_at_argument,
        runtime,
    })
}

pub(crate) fn nonvariadic_namespace_spread_fault_surface(namespace: &str, method: &str) -> bool {
    matches!(
        (namespace, method),
        ("JSON", "parse")
            | ("JSON", "stringify")
            | ("Bytes", "empty")
            | ("Bytes", "encodeUtf8")
            | ("Bytes", "fromArray")
            | ("Bytes", "fromHex")
            | ("Bytes", "fromBase64")
            | ("Bytes", "concat")
            | ("ByteBuffer", "allocate")
            | ("ByteBuffer", "fromBytes")
            | ("Encoding", "utf8Encode")
            | ("Encoding", "utf8Decode")
            | ("Encoding", "hexEncode")
            | ("Encoding", "hexDecode")
            | ("Encoding", "base64Encode")
            | ("Encoding", "base64Decode")
            | ("Map", "new")
    )
}

pub(crate) fn append_rendered_spread_tail(
    tail: &[RenderedSpreadTailArg],
    target: &str,
    rendered: &mut String,
) {
    for arg in tail {
        match arg {
            RenderedSpreadTailArg::Positional(value) => {
                rendered.push_str(&format!("{target}.push({value}); "));
            }
            RenderedSpreadTailArg::Spread { value, span } => {
                rendered.push_str(&format!(
                    "call_spread_extend(&mut {target}, {value}, {span})?; "
                ));
            }
        }
    }
}

pub(crate) fn render_named_call_args(named: &[RenderedNamedArg]) -> String {
    named
        .iter()
        .map(|arg| format!("({:?}.to_string(), {})", arg.name, arg.value))
        .collect::<Vec<_>>()
        .join(", ")
}

impl RenderedCallArgs {
    pub(crate) fn value_call(&self, callee_rs: &str, leading_positional: &[&str]) -> String {
        match self {
            Self::OrderFault(fault) => format!("{{ let _ = {callee_rs}; {fault} }}"),
            Self::Static {
                positional,
                named,
                call_span,
            } => {
                let positional = leading_positional
                    .iter()
                    .copied()
                    .chain(positional.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(", ");
                if named.is_empty() {
                    return format!(
                        "call_value({callee_rs}, vec![{positional}], cx.clone(), {call_span}).await?"
                    );
                }
                let named = render_named_call_args(named);
                format!(
                    "call_value_named({callee_rs}, vec![{positional}], vec![{named}], cx.clone(), {call_span}).await?"
                )
            }
            Self::Spread {
                prefix,
                tail,
                named,
                first_spread_span,
                call_span,
            } => {
                let positional = leading_positional
                    .iter()
                    .copied()
                    .chain(prefix.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut spread = String::from("{ let mut __sp: Vec<Value> = Vec::new(); ");
                append_rendered_spread_tail(tail, "__sp", &mut spread);
                spread.push_str("Value::array(__sp) }");
                if named.is_empty() {
                    return format!(
                        "call_value_spread({callee_rs}, vec![{positional}], {spread}, cx.clone(), {call_span}, {first_spread_span}).await?"
                    );
                }
                let named = render_named_call_args(named);
                format!(
                    "call_value_spread_named(SpreadNamedCall::new({callee_rs}, vec![{positional}], {spread}, vec![{named}], {call_span}, {first_spread_span}), cx.clone()).await?"
                )
            }
        }
    }

    pub(crate) fn arity_fault(&self, target: &str) -> String {
        match self {
            Self::OrderFault(fault) => fault.clone(),
            Self::Static { .. } => unreachable!("arity fault projection requires spread arguments"),
            Self::Spread {
                prefix,
                tail,
                named,
                call_span,
                ..
            } => {
                let mut spread = format!("let mut {target}: Vec<Value> = Vec::new(); ");
                append_rendered_spread_tail(tail, target, &mut spread);
                for arg in named {
                    spread.push_str(&format!("let _ = {}; ", arg.value));
                }
                format!(
                    "{{ let _: Vec<Value> = vec![{}]; {spread}let _ = {target}; let __v: Value = return Err(fault(codes::GUARD_ARITY, {:?}, {call_span})); __v }}",
                    prefix.join(", "),
                    "spread arguments require a variadic parameter (§5)"
                )
            }
        }
    }

    pub(crate) fn resource_call(
        &self,
        method: &str,
        leading_positional: &[&str],
        member_span: &str,
        call_span: &str,
    ) -> String {
        match self {
            Self::OrderFault(fault) => fault.clone(),
            Self::Static {
                positional, named, ..
            } => {
                let positional = leading_positional
                    .iter()
                    .copied()
                    .chain(positional.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(", ");
                if named.is_empty() {
                    return format!(
                        "call_resource_method(&*cx.host(), __recv, {method:?}, vec![{positional}], {member_span}, {call_span})?"
                    );
                }
                let named = render_named_call_args(named);
                format!(
                    "call_resource_method_named(&*cx.host(), __recv, {method:?}, vec![{positional}], vec![{named}], {member_span}, {call_span})?"
                )
            }
            Self::Spread { .. } => self.arity_fault("__tpz_resource_spread"),
        }
    }

    pub(crate) fn method_call(
        &self,
        method: &str,
        leading_positional: &[&str],
        member_span: &str,
        call_span: &str,
    ) -> String {
        match self {
            Self::OrderFault(fault) => fault.clone(),
            Self::Static {
                positional, named, ..
            } => {
                let positional = leading_positional
                    .iter()
                    .copied()
                    .chain(positional.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(", ");
                if named.is_empty() {
                    return format!(
                        "call_method(__recv, {method:?}, vec![{positional}], {member_span}, {call_span})?"
                    );
                }
                let named = render_named_call_args(named);
                format!(
                    "call_method_named(__recv, {method:?}, vec![{positional}], vec![{named}], {member_span}, {call_span})?"
                )
            }
            Self::Spread { .. } => self.arity_fault("__tpz_method_spread"),
        }
    }

    pub(crate) fn all_positional(&self) -> Option<&[String]> {
        match self {
            Self::Static {
                positional, named, ..
            } if named.is_empty() => Some(positional),
            Self::OrderFault(_) | Self::Static { .. } | Self::Spread { .. } => None,
        }
    }
}

pub(crate) fn render_call_args(
    args: &[CallArg],
    ctx: ExprEmitContext<'_, '_, '_>,
    call_span: Span,
    argument_shape_error: &'static str,
) -> Result<RenderedCallArgs, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    if positional_after_named(args) {
        return Ok(RenderedCallArgs::OrderFault(emit_call_arg_order_fault(
            args, src, aliases, locals, in_loop, call_span,
        )?));
    }
    let Some(first_spread) = args
        .iter()
        .position(|arg| matches!(arg, CallArg::Spread(_)))
    else {
        let mut positional = Vec::new();
        let mut named = Vec::new();
        for arg in args {
            match arg {
                CallArg::Positional(expr) => {
                    positional.push(emit_expr(expr, src, aliases, locals, in_loop)?);
                }
                CallArg::Named { name, value } => named.push(RenderedNamedArg {
                    name: text(src, name.span).to_string(),
                    value: emit_expr(value, src, aliases, locals, in_loop)?,
                }),
                CallArg::Spread(_) => unreachable!("spread absence checked"),
            }
        }
        return Ok(RenderedCallArgs::Static {
            positional,
            named,
            call_span: emit_span(call_span),
        });
    };
    let mut prefix = Vec::new();
    for arg in &args[..first_spread] {
        let CallArg::Positional(expr) = arg else {
            return Err(EmitError::unsupported(argument_shape_error));
        };
        prefix.push(emit_expr(expr, src, aliases, locals, in_loop)?);
    }

    let region_end = args[first_spread..]
        .iter()
        .position(|arg| matches!(arg, CallArg::Named { .. }))
        .map(|index| index + first_spread)
        .unwrap_or(args.len());
    let CallArg::Spread(first) = &args[first_spread] else {
        unreachable!("first_spread indexes a spread")
    };
    let mut tail = Vec::new();
    for arg in &args[first_spread..region_end] {
        match arg {
            CallArg::Positional(expr) => tail.push(RenderedSpreadTailArg::Positional(emit_expr(
                expr, src, aliases, locals, in_loop,
            )?)),
            CallArg::Spread(expr) => tail.push(RenderedSpreadTailArg::Spread {
                value: emit_expr(expr, src, aliases, locals, in_loop)?,
                span: emit_span(expr.span),
            }),
            CallArg::Named { .. } => unreachable!("region_end stops at the first named arg"),
        }
    }
    let mut named = Vec::new();
    for arg in &args[region_end..] {
        let CallArg::Named { name, value } = arg else {
            return Err(EmitError::unsupported(argument_shape_error));
        };
        named.push(RenderedNamedArg {
            name: text(src, name.span).to_string(),
            value: emit_expr(value, src, aliases, locals, in_loop)?,
        });
    }
    Ok(RenderedCallArgs::Spread {
        prefix,
        tail,
        named,
        first_spread_span: emit_span(first.span),
        call_span: emit_span(call_span),
    })
}

pub(crate) fn emit_nonvariadic_namespace_spread_fault(
    args: &[CallArg],
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    locals: &[(String, Bind)],
    in_loop: bool,
    call_span: Span,
) -> Result<String, EmitError> {
    Ok(render_call_args(
        args,
        ExprEmitContext {
            src,
            aliases,
            locals,
            in_loop,
        },
        call_span,
        "namespace call argument shape",
    )?
    .arity_fault("__tpz_ns_spread"))
}

pub(crate) fn emit_call_expr(
    expr: &Expr,
    callee: &Expr,
    args: &[CallArg],
    type_args: &[Type],
    ctx: ExprEmitContext<'_, '_, '_>,
) -> Result<String, EmitError> {
    let ExprEmitContext {
        src,
        aliases,
        locals,
        in_loop,
    } = ctx;
    Ok({
        if let ExprKind::Member { object, field } = &callee.kind
            && let ExprKind::Ident = &object.kind
            && text(src, object.span) == "JSON"
            && matches!(text(src, field.span), "parseAs" | "decode")
            && !locals.iter().any(|(n, _)| n == "JSON")
        {
            return emit_typed_json_call(expr, field, args, type_args, ctx);
        }
        // §17 a module namespace import (`import math`) is a RECORD value whose fields
        // are the module exports. Calls such as `math.add(b: 2, a: 1)` must lower as
        // ordinary value calls, including named/spread binding through the same helper
        // as lambdas and selected imports. This has to run before receiver-mutator
        // dispatch: export names like `add` can overlap collection mutator names, and
        // the namespace record field must win.
        if let ExprKind::Member { object, field } = &callee.kind
            && let ExprKind::Ident = &object.kind
            && matches!(
                lookup_bind(locals, text(src, object.span)),
                Some(Bind::Namespace)
            )
        {
            return emit_module_namespace_call(expr, callee, object, field, args, ctx);
        }
        // §12 optional CALL `obj?.field(args)` (the callee is an
        // `OptionalAccess`) has DISTINCT semantics the generic call path
        // does not model, mirroring the interpreter's `KOptionalCall`:
        // evaluate the RECEIVER, then `None`/`null` short-circuit WITHOUT
        // evaluating the field or the args; a `Some(inner)` receiver
        // accesses `inner.field`, CALLS it, and re-wraps the CALL RESULT
        // (`wrap_optional`); any other receiver accesses + calls directly
        // (no re-wrap). The member access and the call share ONE span (the
        // interpreter threads a single span to both). The inner dispatch is
        // the SAME as a non-optional bound-method call (`member_value`-first
        // → `call_value` for a record-field closure, else `call_method` for
        // a read-only builtin); because the wholesale call was refused
        // before, this handles EVERY field name, not just `get`/`scalars`.
        // The RESOURCE methods (`read`/`write`/`close`) dispatch THROUGH THE HOST
        // (`call_resource_method`) on the non-short-circuit branch, like a direct
        // `file.read()` but re-wrapped on a `Some` receiver. The MUTATING methods
        // (`push`/`insert`/`add`/`remove`) need a `mut`-root, keyed on the receiver
        // path's root (`mutation_root`), applied on the non-short-circuit
        // (`Some`/non-null) branch — exactly the interpreter, whose `None`/`null`
        // short-circuit precedes the mut-check. A bare optional ACCESS `obj?.field`
        // is the `OptionalAccess` arm.
        if let ExprKind::OptionalAccess { object, field } = &callee.kind {
            return emit_optional_call_expr(expr, object, field, args, ctx);
        }
        // §9/§22.2 an in-place MUTATOR call — `arr.push(x)`, `map.insert(k,v)`,
        // `set.add(x)`, `coll.remove(x)`. The interpreter resolves the member FIRST
        // (`member_value` — a record field named like a mutator SHADOWS, exactly as a
        // field access), and only a COLLECTION receiver rooted at a `let mut` binding
        // reaches the bound mutator (`require_mut_root`). Mirror that: gate on a `let
        // mut` LOCAL receiver (the static `require_mut_root` analog — an immutable/unbound
        // or non-local receiver is refused; the checker rejects an immutable mutation, and
        // the shared `Rc<RefCell>` means the receiver clone still mutates the one
        // collection), then lower to the SAME `member_value`-first dispatch the optional
        // path uses: `call_value` for a record-field closure, else the shared `call_method`
        // leaf (which both engines call for the mutators, so they cannot drift). The
        // mutation + unhashable-key faults fall at the CALL span; the wrong-type
        // `no_member_fault` and a too-few/many `arity` fault fall where `call_method` raises
        // them (member span / call span). The READ-ONLY methods (`get`/`scalars`) ride the
        // same `member_value`-first dispatch in the bound-method-call block below (they need
        // no `mut`-root, so they are not gated here).
        if let ExprKind::Member { object, field } = &callee.kind
                // §6 (v5.4) `clear` is also a `call_method`-leaf mutator (0 args → Unit),
                // so it rides this exact path. (`update` is a CALLBACK mutator with its own
                // inline lowering below — NOT here.) The v5.4 array mutation API's SIMPLE
                // (non-callback) mutators ride here too: `pop`/`reverse`/`removeAt`/`sort`
                // (0/1 args) — they are `call_method` leaves. `insert` (2 args, out-of-range
                // fault) also rides here. The CALLBACK array mutators `sortBy`/`retain` have
                // their own inline lowering below (like `update`).
                && is_call_method_collection_mutator_name(text(src, field.span))
        {
            return emit_in_place_collection_mutator_call(expr, callee, object, field, args, ctx);
        }
        // §6 (v5.4) `m.update(k, initial, f)` — a CALLBACK mutator (3 args). It cannot
        // ride `call_method` (that would force `f` eagerly), so it lowers INLINE, with
        // the SAME `mut`-root gate the other mutators use (member_value-first shadow;
        // an immutable LOCAL root faults GUARD_IMMUTABLE after the type gate). The
        // callback fault, the unhashable-key fault, and the present/absent slot
        // semantics are owned by the shared callback-map-update transition.
        if let ExprKind::Member { object, field } = &callee.kind
            && text(src, field.span) == "update"
        {
            return emit_map_update_call(expr, callee, object, args, ctx);
        }
        // §6 (v5.4) `xs.sortBy(f)` / `xs.retain(f)` — CALLBACK array mutators (1 arg).
        // Like `update` they cannot ride `call_method` (it would force `f` eagerly),
        // so they use dedicated callback drivers with the SAME `mut`-root gate (member_value-first shadow;
        // an immutable LOCAL root faults GUARD_IMMUTABLE BEFORE any eval). The callback
        // fault + the in-place write-back + (sortBy) the order-comparability fault are
        // byte-identical to the interpreter's `ArrSortBy` and `ArrRetain`.
        // (The RETURN-new `sortedBy`/`filter` live in the
        // receiver-HOF block below — these IN-PLACE mutators are gated here.)
        if let ExprKind::Member { object, field } = &callee.kind
            && matches!(text(src, field.span), "sortBy" | "retain")
        {
            return emit_array_callback_mutator_call(expr, callee, object, field, args, ctx);
        }
        if let ExprKind::Member { object, field } = &callee.kind
            && let Some(constructor) =
                try_emit_enum_constructor_call(expr, object, field, args, ctx)?
        {
            return Ok(constructor);
        }
        // §4 (v5.4) a PROTOCOL static dispatch `Show.show(x)` / `Order.compare(a,
        // b)` — the callee is `head.method` where `head` is a declared protocol NOT
        // shadowed by a local. Emit the args, then dispatch on arg0's RUNTIME
        // nominal id: a MANUAL impl method `("{protocol}<{id}>", method)` wins (via
        // `call_value`); else the DERIVED `builtin_protocol_dispatch` leaf
        // (Show→render, Eq→values_equal, Order→values_compare) — byte-identical to
        // the interpreter's `KProtocolCall` (run≡build, incl. `--unchecked`). The
        // static heads/ctors below this point are never protocols (distinct names).
        if let ExprKind::Member { object, field } = &callee.kind
            && let ExprKind::Ident = &object.kind
            && !locals.iter().any(|(n, _)| n == text(src, object.span))
            && aliases.protocols.contains(text(src, object.span))
            && args.iter().all(|a| matches!(a, CallArg::Positional(_)))
        {
            return emit_protocol_static_call(expr, object, field, args, ctx);
        }
        if let ExprKind::Member { object, field } = &callee.kind
            && matches!(&object.kind, ExprKind::Ident)
            && !locals
                .iter()
                .any(|(name, _)| name == text(src, object.span))
        {
            let namespace = text(src, object.span);
            let member = text(src, field.span);
            if let Some(builtin) = Builtin::static_namespace(namespace, member) {
                return emit_static_namespace_call(expr, object, field, args, builtin, ctx);
            }
        }
        // §4 (v5.4) a user RECEIVER-METHOD call `recv.m(args)`: when `m` is a
        // declared method name (for SOME type), dispatch on the receiver's RUNTIME
        // nominal id — read it, look up `(id, m)` in the method registry, and
        // `call_value` the closure with `recv` prepended as the first argument
        // (STATIC dispatch → a free call). FIELD-FIRST precedence: a record field
        // named `m` SHADOWS (via the shared `member_value` leaf) and is called as a
        // value (the checker rejects a field/method name collision, so this is the
        // `--unchecked` safety arm). A non-nominal receiver or an absent method
        // falls back to ordinary member access (`call_method`/`check_member_method`)
        // so a builtin method / an absent-member fault matches the interpreter
        // byte-for-byte (run≡build). This is BEFORE the bound-builtin matches so a
        // record method named like a builtin (`map`) wins, as the interpreter's
        // nominal-id-first dispatch does. The enum/newtype-construct + static heads
        // above already returned, so this never shadows them. Named/defaulted/spread
        // args lower through the same call helpers as ordinary functions, with the
        // receiver prepended as `self`.
        if let ExprKind::Member { object, field } = &callee.kind
            && aliases.method_names.contains(text(src, field.span))
        {
            return emit_user_receiver_method_call(expr, callee, object, field, args, ctx);
        }
        // Unshadowed free identifiers own newtype, prelude, intrinsic, and
        // free-builtin dispatch in one route. Locals remain ordinary value calls.
        if matches!(callee.kind, ExprKind::Ident)
            && !locals
                .iter()
                .any(|(name, _)| name == text(src, callee.span))
        {
            return emit_unshadowed_ident_call(expr, callee, args, ctx);
        }
        if let ExprKind::Member { object, field } = &callee.kind
            && is_read_only_receiver_method(text(src, field.span))
        {
            return emit_read_only_receiver_call(expr, callee, object, field, args, ctx);
        }
        // §22 receiver HOFs `xs.filter(f)` / `xs.map(f)` / `xs.reduce(init, f)`:
        // `member_value`-first lets a record field of that name SHADOW the HOF
        // (then `call_value` invokes it with the arg(s)); otherwise the receiver is
        // the collection and the same callback-HOF driver as the free forms runs.
        // Eval order matches: receiver, member resolution, then the args L→R.
        if let ExprKind::Member { object, field } = &callee.kind
            && matches!(
                text(src, field.span),
                // §6 (v5.4) `mapValues` joins the receiver-HOF family (Map-only); `filter`
                // now dispatches Array vs Map at RUNTIME (the emitter can't know the type).
                "filter" | "map" | "reduce" | "sortedBy" | "mapValues"
            )
        {
            return emit_receiver_hof_call(expr, callee, object, field, args, ctx);
        }
        // §22 `opt.flatMap(f)` — Option-only (no Array form), so it does NOT share
        // the array/option `map` dispatch. member_value-FIRST (a record field named
        // `flatMap` shadows); otherwise LAZY: `Some(v)->f(v)` directly (f already
        // returns an Option, no re-wrap), `None->None` (f dropped). `__f` is bound
        // BEFORE the match so the callback arg is evaluated once even for `None`.
        if let ExprKind::Member { object, field } = &callee.kind
            && text(src, field.span) == "flatMap"
        {
            return emit_flat_map_call(expr, callee, object, args, ctx);
        }
        // The lazy Option-to-Result bridge owns spread refusal, builtin argument
        // binding, record-field shadowing, callback laziness, and exact fault order.
        if let ExprKind::Member { object, field } = &callee.kind
            && text(src, field.span) == "okOrElse"
            && (args.iter().any(|arg| matches!(arg, CallArg::Spread(_)))
                || !positional_after_named(args))
        {
            return emit_ok_or_else_call(expr, callee, object, field, args, ctx);
        }
        // §22.3 bound RESOURCE method CALLS — `file.read()` / `file.write(s)` /
        // `file.close()` on a `Value::Resource`. Like the read-only get/scalars
        // block, MIRROR the interpreter's `member_access`: `member_value` first
        // (a record field of that name SHADOWS → `call_value`), else the shared
        // `call_resource_method` leaf, which both engines call THROUGH THE HOST on
        // `cx` so the effect boundary cannot drift. These need no `mut`-root (the
        // resource is the effect anchor, not a mutable binding), so the receiver is
        // any expression. TWO spans: the member expression's (the no-method /
        // record-no-field fault) and the call's (arity / write type fault).
        if let ExprKind::Member { object, field } = &callee.kind
            && is_resource_receiver_method(text(src, field.span))
        {
            return emit_resource_method_call(expr, callee, object, field, args, ctx);
        }
        // Remaining callees are ordinary values. The shared argument lowering
        // preserves callee-first evaluation, source-order arguments, named slots,
        // variadic spread flattening, and their runtime faults.
        let callee_rs = emit_expr(callee, src, aliases, locals, in_loop)?;
        emit_call_value_with_args(&callee_rs, &[], args, ctx, expr.span)?
    })
}
