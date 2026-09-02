use super::*;

pub(super) fn emit_boxed_boundary_expr(
    expr: &Expr,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<String, EmitError> {
    match emit_expr(expr, ctx, scope) {
        Ok(low) => return Ok(low.ty.box_expr(&low.rs)),
        Err(e) if e.is_native_decline() => {}
        Err(e) => return Err(e),
    }
    emit_boxed_boundary_expr_inner(expr, ctx, scope).map_err(|e| e.at(expr.span))
}

pub(super) fn emit_boxed_boundary_expr_inner(
    expr: &Expr,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<String, EmitError> {
    match &expr.kind {
        ExprKind::Null => Ok("Value::Null".to_string()),
        ExprKind::Ident => {
            let name = text(ctx.src, expr.span);
            if name == "None" && scope.iter().all(|local| local.name != name) {
                return Ok("Value::None".to_string());
            }
            let local = scope
                .iter()
                .rev()
                .find(|local| local.name == name)
                .ok_or_else(|| decline("a boxed boundary free identifier"))?;
            if local.is_boxed_carrier() {
                return Ok(format!("{}.clone()", mangle(name)));
            }
            Err(decline("a boxed boundary identifier"))
        }
        ExprKind::String(lit) if lit.tag.is_none() => {
            let mut stmts = String::new();
            for part in &lit.parts {
                match part {
                    StringPart::Text(span) => {
                        let mut decoded = String::new();
                        decode_escapes(text(ctx.src, *span), &mut decoded, *span)
                            .map_err(|_| EmitError::malformed_literal("string escape"))?;
                        stmts.push_str(&format!("__s.push_str({decoded:?}); "));
                    }
                    StringPart::Interpolation(e) => {
                        let e = emit_boxed_boundary_expr(e, ctx, scope)?;
                        stmts.push_str(&format!("__s.push_str(&render(&({e}))); "));
                    }
                }
            }
            Ok(format!(
                "{{ let mut __s = String::new(); {stmts}Value::str(__s) }}"
            ))
        }
        ExprKind::String(lit) => {
            let tag_span = lit.tag.expect("untagged string handled above");
            let tag = text(ctx.src, tag_span);
            let mut parts: Vec<String> = Vec::new();
            let mut cur = String::new();
            let mut values: Vec<String> = Vec::new();
            for part in &lit.parts {
                match part {
                    StringPart::Text(span) => {
                        decode_escapes(text(ctx.src, *span), &mut cur, *span)
                            .map_err(|_| EmitError::malformed_literal("string escape"))?;
                    }
                    StringPart::Interpolation(e) => {
                        parts.push(std::mem::take(&mut cur));
                        values.push(emit_boxed_boundary_expr(e, ctx, scope)?);
                    }
                }
            }
            parts.push(cur);
            let parts = parts
                .iter()
                .map(|part| format!("{part:?}.to_string()"))
                .collect::<Vec<_>>()
                .join(", ");
            Ok(format!(
                "make_template({tag:?}.to_string(), vec![{parts}], vec![{}])",
                values.join(", ")
            ))
        }
        ExprKind::Array(elements) => {
            let has_spread = elements
                .iter()
                .any(|element| matches!(element, ast::ArrayElement::Spread(_)));
            if !has_spread {
                let mut boxed = Vec::with_capacity(elements.len());
                for element in elements {
                    let ast::ArrayElement::Expr(e) = element else {
                        unreachable!("spread checked above")
                    };
                    boxed.push(emit_boxed_boundary_expr(e, ctx, scope)?);
                }
                return Ok(format!("Value::array(vec![{}])", boxed.join(", ")));
            }
            let span = emit_span(expr.span);
            let mut stmts = String::new();
            for element in elements {
                match element {
                    ast::ArrayElement::Expr(e) => {
                        let value = emit_boxed_boundary_expr(e, ctx, scope)?;
                        stmts.push_str(&format!("__items.push({value}); "));
                    }
                    ast::ArrayElement::Spread(e) => {
                        let value = emit_boxed_boundary_expr(e, ctx, scope)?;
                        stmts.push_str(&format!(
                            "array_spread_extend(&mut __items, {value}, {span})?; "
                        ));
                    }
                }
            }
            Ok(format!(
                "{{ let mut __items: Vec<Value> = Vec::new(); {stmts}Value::array(__items) }}"
            ))
        }
        ExprKind::SetLiteral(elements) => {
            let mut boxed = Vec::with_capacity(elements.len());
            for element in elements {
                boxed.push(emit_boxed_boundary_expr(element, ctx, scope)?);
            }
            Ok(format!(
                "builtin_set_of(vec![{}], {})?",
                boxed.join(", "),
                emit_span(expr.span)
            ))
        }
        ExprKind::MapLiteral(entries) => {
            let mut pairs = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                let key = emit_boxed_boundary_expr(key, ctx, scope)?;
                let value = emit_boxed_boundary_expr(value, ctx, scope)?;
                pairs.push(format!("({key}, {value})"));
            }
            Ok(format!(
                "builtin_map_of(vec![{}], {})?",
                pairs.join(", "),
                emit_span(expr.span)
            ))
        }
        ExprKind::RecordLiteral { fields } => {
            let mut pairs = Vec::with_capacity(fields.len());
            for field in fields {
                let name = text(ctx.src, field.name.span);
                let value = emit_boxed_boundary_expr(&field.value, ctx, scope)?;
                pairs.push(format!("({name:?}.to_string(), {value})"));
            }
            Ok(format!("Value::record([{}])", pairs.join(", ")))
        }
        ExprKind::RecordUpdate {
            base,
            spread: None,
            fields,
        } => {
            let base = emit_boxed_boundary_expr(base, ctx, scope)?;
            let span = emit_span(expr.span);
            let mut field_lets = String::new();
            let mut pairs = Vec::with_capacity(fields.len());
            for (idx, field) in fields.iter().enumerate() {
                let name = text(ctx.src, field.name.span);
                let value = emit_boxed_boundary_expr(&field.value, ctx, scope)?;
                field_lets.push_str(&format!("let __f{idx}: Value = {value}; "));
                pairs.push(format!("({name:?}.to_string(), __f{idx})"));
            }
            Ok(format!(
                "{{ let __base = record_update_base({base}, {span})?; {field_lets}record_update_merge(__base, vec![{}], {span})? }}",
                pairs.join(", ")
            ))
        }
        ExprKind::RecordUpdate {
            spread: Some(_), ..
        } => Err(decline("a boxed boundary nominal record update")),
        ExprKind::Call { callee, args, .. } => {
            if let Some(rendered) = emit_boxed_byte_call(callee, args, ctx, scope, expr.span)? {
                Ok(rendered)
            } else {
                emit_boxed_boundary_call(callee, args, ctx, scope, expr.span)
            }
        }
        ExprKind::Block(block) => emit_boxed_boundary_block(block, ctx, scope),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            let cond = emit_boxed_boundary_expr(cond, ctx, scope)?;
            let then_rs = emit_boxed_boundary_block(then_block, ctx, scope)?;
            let else_rs = match else_branch {
                Some(branch) if matches!(branch.kind, ExprKind::If { .. }) => {
                    emit_boxed_boundary_expr(branch, ctx, scope)?
                }
                Some(branch) => {
                    let value = match &branch.kind {
                        ExprKind::Block(block) => {
                            emit_boxed_boundary_block_value(block, ctx, scope)?
                        }
                        _ => emit_boxed_boundary_expr(branch, ctx, scope)?,
                    };
                    format!("{{ {value} }}")
                }
                None => "{ Value::Unit }".to_string(),
            };
            Ok(format!(
                "if condition_bool(&({cond}), \"if\", {})? {then_rs} else {else_rs}",
                emit_span(expr.span)
            ))
        }
        ExprKind::Index { object, index } => {
            let object = emit_boxed_boundary_expr(object, ctx, scope)?;
            let index = emit_boxed_boundary_expr(index, ctx, scope)?;
            Ok(format!(
                "index_value({object}, {index}, {})?",
                emit_span(expr.span)
            ))
        }
        ExprKind::Member { object, field } => {
            let object = emit_boxed_boundary_expr(object, ctx, scope)?;
            let field = text(ctx.src, field.span);
            Ok(format!(
                "member_value_required(&({object}), {field:?}, {})?",
                emit_span(expr.span)
            ))
        }
        ExprKind::OptionalAccess { object, field } => {
            let object = emit_boxed_boundary_expr(object, ctx, scope)?;
            let field = text(ctx.src, field.span);
            Ok(format!(
                "optional_member({object}, {field:?}, {})?",
                emit_span(expr.span)
            ))
        }
        ExprKind::Binary {
            op: op @ (BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce),
            lhs,
            rhs,
        } => {
            let lhs = emit_boxed_boundary_expr(lhs, ctx, scope)?;
            let rhs = emit_boxed_boundary_expr(rhs, ctx, scope)?;
            let op = match op {
                BinaryOp::And => "BinaryOp::And",
                BinaryOp::Or => "BinaryOp::Or",
                BinaryOp::Coalesce => "BinaryOp::Coalesce",
                _ => unreachable!(),
            };
            Ok(format!(
                "{{ let __lhs = {lhs}; match short_circuit_lhs(__lhs, {op}, {})? {{ \
                 Some(__v) => __v, \
                 None => {rhs}, \
                 }} }}",
                emit_span(expr.span)
            ))
        }
        ExprKind::Paren(inner) => emit_boxed_boundary_expr(inner, ctx, scope),
        _ => Err(decline("a boxed boundary expression")),
    }
}

pub(super) fn emit_boxed_boundary_block(
    block: &Block,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<String, EmitError> {
    let value = emit_boxed_boundary_block_value(block, ctx, scope)?;
    Ok(format!("{{ {value} }}"))
}

pub(super) fn emit_boxed_boundary_block_value(
    block: &Block,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<String, EmitError> {
    if !block.stmts.is_empty() {
        return Err(decline("a boxed boundary block with statements"));
    }
    Ok(match block.tail.as_deref() {
        Some(tail) => emit_boxed_boundary_expr(tail, ctx, scope)?,
        None => "Value::Unit".to_string(),
    })
}

pub(super) fn emit_boxed_boundary_call(
    callee: &Expr,
    args: &[ast::CallArg],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    span: Span,
) -> Result<String, EmitError> {
    if let ExprKind::Ident = &callee.kind {
        let name = text(ctx.src, callee.span);
        if scope.iter().all(|local| local.name != name) {
            return match name {
                "Some" | "Ok" | "Err" => {
                    let [ast::CallArg::Positional(arg)] = args else {
                        return Err(decline("a boxed boundary constructor call shape"));
                    };
                    let value = emit_boxed_boundary_expr(arg, ctx, scope)?;
                    let variant = match name {
                        "Some" => "Some",
                        "Ok" => "Ok",
                        "Err" => "Err",
                        _ => unreachable!(),
                    };
                    Ok(format!("Value::{variant}(Rc::new({value}))"))
                }
                "toInt" => emit_boxed_boundary_leaf_call(
                    "builtin_to_int",
                    args,
                    &["text"],
                    ctx,
                    scope,
                    span,
                ),
                "toIntRadix" => emit_boxed_boundary_leaf_call(
                    "builtin_to_int_radix",
                    args,
                    &["text", "radix"],
                    ctx,
                    scope,
                    span,
                ),
                "fromCodePoint" => emit_boxed_boundary_leaf_call(
                    "builtin_from_code_point",
                    args,
                    &["n"],
                    ctx,
                    scope,
                    span,
                ),
                "toFloat" => emit_boxed_boundary_leaf_call(
                    "builtin_to_float",
                    args,
                    &["n"],
                    ctx,
                    scope,
                    span,
                ),
                _ => Err(decline("a boxed boundary non-constructor call")),
            };
        }
    }

    if let ExprKind::OptionalAccess { object, field } = &callee.kind {
        let member = text(ctx.src, field.span);
        if !is_boxed_boundary_receiver_method(member) {
            return Err(decline("a boxed boundary optional member call"));
        }
        let object = emit_boxed_boundary_expr(object, ctx, scope)?;
        let some_call = emit_boxed_boundary_method_dispatch(
            member,
            "(*__inner).clone()",
            args,
            ctx,
            scope,
            callee.span,
            span,
        )?;
        let other_call = emit_boxed_boundary_method_dispatch(
            member,
            "__other",
            args,
            ctx,
            scope,
            callee.span,
            span,
        )?;
        return Ok(format!(
            "{{ let __obj = {object}; match __obj {{ \
             Value::None => Value::None, \
             Value::Null => Value::Null, \
             Value::Some(__inner) => wrap_optional({some_call}), \
             __other => {other_call}, \
             }} }}"
        ));
    }

    let ExprKind::Member { object, field } = &callee.kind else {
        return Err(decline("a boxed boundary non-member call"));
    };
    let member = text(ctx.src, field.span);
    if let ExprKind::Ident = &object.kind {
        let namespace = text(ctx.src, object.span);
        if scope.iter().all(|local| local.name != namespace) {
            match (namespace, member) {
                ("Array", "of") | ("Set", "of") => {
                    let has_spread = args
                        .iter()
                        .any(|arg| matches!(arg, ast::CallArg::Spread(_)));
                    if !has_spread {
                        let mut rendered = Vec::with_capacity(args.len());
                        for arg in args {
                            let ast::CallArg::Positional(expr) = arg else {
                                return Err(decline(
                                    "a boxed boundary collection constructor argument shape",
                                ));
                            };
                            rendered.push(emit_boxed_boundary_expr(expr, ctx, scope)?);
                        }
                        return Ok(if namespace == "Array" {
                            format!("Value::array(vec![{}])", rendered.join(", "))
                        } else {
                            format!(
                                "builtin_set_of(vec![{}], {})?",
                                rendered.join(", "),
                                emit_span(span)
                            )
                        });
                    }
                    let mut stmts = String::new();
                    let span_rs = emit_span(span);
                    for arg in args {
                        match arg {
                            ast::CallArg::Positional(expr) => {
                                let value = emit_boxed_boundary_expr(expr, ctx, scope)?;
                                stmts.push_str(&format!("__items.push({value}); "));
                            }
                            ast::CallArg::Spread(expr) => {
                                let value = emit_boxed_boundary_expr(expr, ctx, scope)?;
                                stmts.push_str(&format!(
                                    "call_spread_extend(&mut __items, {value}, {span_rs})?; "
                                ));
                            }
                            ast::CallArg::Named { .. } => {
                                return Err(decline(
                                    "a boxed boundary collection constructor argument shape",
                                ));
                            }
                        }
                    }
                    return Ok(if namespace == "Array" {
                        format!(
                            "{{ let mut __items: Vec<Value> = Vec::new(); {stmts}Value::array(__items) }}"
                        )
                    } else {
                        format!(
                            "{{ let mut __items: Vec<Value> = Vec::new(); {stmts}builtin_set_of(__items, {span_rs})? }}"
                        )
                    });
                }
                ("Map", "new") => {
                    if !args.is_empty() {
                        return Err(decline("a boxed boundary Map.new argument shape"));
                    }
                    return Ok("builtin_map_new()".to_string());
                }
                ("Map", "ofEntries") => {
                    return emit_boxed_boundary_leaf_call(
                        "builtin_map_of_entries",
                        args,
                        &["entries"],
                        ctx,
                        scope,
                        span,
                    );
                }
                _ => {}
            }
        }
        if namespace == "Math" && scope.iter().all(|local| local.name != namespace) {
            let (leaf, params): (&str, &[&str]) = match member {
                "sqrt" => ("builtin_math_sqrt", &["x"]),
                "parseFloat" => ("builtin_math_parse_float", &["s"]),
                _ => return Err(decline("a boxed boundary Math static call")),
            };
            let args = boxed_boundary_args(args, params, ctx.src)?;
            let mut rendered = Vec::with_capacity(args.len());
            for arg in args {
                rendered.push(emit_boxed_boundary_expr(arg, ctx, scope)?);
            }
            return Ok(format!(
                "{leaf}({}, {})?",
                rendered.join(", "),
                emit_span(span)
            ));
        }
        if namespace == "JSON" && scope.iter().all(|local| local.name != namespace) {
            match member {
                "stringify" => {
                    let args = boxed_boundary_args(args, &["value"], ctx.src)?;
                    let value = emit_boxed_boundary_expr(args[0], ctx, scope)?;
                    return Ok(format!("builtin_json_stringify({value})"));
                }
                "parse" => {
                    let args = boxed_boundary_args(args, &["text"], ctx.src)?;
                    let text = emit_boxed_boundary_expr(args[0], ctx, scope)?;
                    return Ok(format!("builtin_json_parse({text}, {})?", emit_span(span)));
                }
                _ => return Err(decline("a boxed boundary JSON static call")),
            }
        }
        if namespace == "Path" && scope.iter().all(|local| local.name != namespace) {
            let (leaf, params): (&str, &[&str]) = match member {
                "from" => ("builtin_path_from", &["text"]),
                "cwdRelative" => ("builtin_path_cwd_relative", &["text"]),
                "project" => ("builtin_path_project", &["text"]),
                _ => return Err(decline("a boxed boundary Path static call")),
            };
            return emit_boxed_boundary_leaf_call(leaf, args, params, ctx, scope, span);
        }
        if namespace == "Cli" && scope.iter().all(|local| local.name != namespace) {
            let (leaf, params): (&str, &[&str]) = match member {
                "hasFlag" => ("builtin_cli_has_flag", &["args", "name"]),
                "option" => ("builtin_cli_option", &["args", "name"]),
                "options" => ("builtin_cli_options", &["args", "name"]),
                "positionals" => ("builtin_cli_positionals", &["args"]),
                _ => return Err(decline("a boxed boundary Cli static call")),
            };
            return emit_boxed_boundary_leaf_call(leaf, args, params, ctx, scope, span);
        }
        if namespace == "Regex" && scope.iter().all(|local| local.name != namespace) {
            let (leaf, params): (&str, &[&str]) = match member {
                "compile" => ("builtin_regex_compile", &["pattern"]),
                _ => return Err(decline("a boxed boundary Regex static call")),
            };
            return emit_boxed_boundary_leaf_call(leaf, args, params, ctx, scope, span);
        }
        if namespace == "CSV" && scope.iter().all(|local| local.name != namespace) {
            let (leaf, params): (&str, &[&str]) = match member {
                "parse" => ("builtin_csv_parse", &["text"]),
                "parseWithHeader" => ("builtin_csv_parse_with_header", &["text"]),
                "stringify" => ("builtin_csv_stringify", &["rows"]),
                "stringifyWithHeader" => {
                    ("builtin_csv_stringify_with_header", &["rows", "columns"])
                }
                _ => return Err(decline("a boxed boundary CSV static call")),
            };
            return emit_boxed_boundary_leaf_call(leaf, args, params, ctx, scope, span);
        }
        if namespace == "TOML" && scope.iter().all(|local| local.name != namespace) {
            let (leaf, params): (&str, &[&str]) = match member {
                "parse" => ("builtin_toml_parse", &["text"]),
                _ => return Err(decline("a boxed boundary TOML static call")),
            };
            return emit_boxed_boundary_leaf_call(leaf, args, params, ctx, scope, span);
        }
        if namespace == "URL" && scope.iter().all(|local| local.name != namespace) {
            let (leaf, params): (&str, &[&str]) = match member {
                "parse" => ("builtin_url_parse", &["text"]),
                _ => return Err(decline("a boxed boundary URL static call")),
            };
            return emit_boxed_boundary_leaf_call(leaf, args, params, ctx, scope, span);
        }
        if namespace == "Date" && scope.iter().all(|local| local.name != namespace) {
            let (leaf, params): (&str, &[&str]) = match member {
                "fromYmd" => ("builtin_date_from_ymd", &["year", "month", "day"]),
                "parseIso" => ("builtin_date_parse_iso", &["text"]),
                _ => return Err(decline("a boxed boundary Date static call")),
            };
            return emit_boxed_boundary_leaf_call(leaf, args, params, ctx, scope, span);
        }
        if namespace == "BigInt" && scope.iter().all(|local| local.name != namespace) {
            let (leaf, params): (&str, &[&str]) = match member {
                "fromInt" => ("builtin_bigint_from_int", &["n"]),
                "parse" => ("builtin_bigint_parse", &["text", "radix"]),
                _ => return Err(decline("a boxed boundary BigInt static call")),
            };
            return emit_boxed_boundary_leaf_call(leaf, args, params, ctx, scope, span);
        }
        if namespace == "Decimal" && scope.iter().all(|local| local.name != namespace) {
            let (leaf, params): (&str, &[&str]) = match member {
                "fromInt" => ("builtin_decimal_from_int", &["n"]),
                "parse" => ("builtin_decimal_parse", &["text"]),
                _ => return Err(decline("a boxed boundary Decimal static call")),
            };
            return emit_boxed_boundary_leaf_call(leaf, args, params, ctx, scope, span);
        }
        if namespace == "Bytes" && scope.iter().all(|local| local.name != namespace) {
            let (leaf, params): (&str, &[&str]) = match member {
                "empty" => ("builtin_bytes_empty", &[]),
                "encodeUtf8" => ("builtin_bytes_encode_utf8", &["s"]),
                "fromArray" => ("builtin_bytes_from_array", &["values"]),
                "fromHex" => ("builtin_bytes_from_hex", &["s"]),
                "fromBase64" => ("builtin_bytes_from_base64", &["s"]),
                "concat" => ("builtin_bytes_concat", &["a", "b"]),
                _ => return Err(decline("a boxed boundary Bytes static call")),
            };
            let args = boxed_boundary_args(args, params, ctx.src)?;
            let mut rendered = Vec::with_capacity(args.len());
            for arg in args {
                rendered.push(emit_boxed_boundary_expr(arg, ctx, scope)?);
            }
            let call_args = if rendered.is_empty() {
                emit_span(span)
            } else {
                format!("{}, {}", rendered.join(", "), emit_span(span))
            };
            return Ok(format!("{leaf}({call_args})?",));
        }
        if namespace == "Hash" && scope.iter().all(|local| local.name != namespace) {
            let (leaf, params): (&str, &[&str]) = match member {
                "sha256" => ("builtin_hash_sha256", &["data"]),
                "sha512" => ("builtin_hash_sha512", &["data"]),
                "hmacSha256" => ("builtin_hash_hmac_sha256", &["key", "message"]),
                "crc32" => ("builtin_hash_crc32", &["data"]),
                _ => return Err(decline("a boxed boundary Hash static call")),
            };
            let args = boxed_boundary_args(args, params, ctx.src)?;
            let mut rendered = Vec::with_capacity(args.len());
            for arg in args {
                rendered.push(emit_boxed_boundary_expr(arg, ctx, scope)?);
            }
            return Ok(format!(
                "{leaf}({}, {})?",
                rendered.join(", "),
                emit_span(span)
            ));
        }
        if namespace == "Encoding" && scope.iter().all(|local| local.name != namespace) {
            let (leaf, params): (&str, &[&str]) = match member {
                "utf8Encode" => ("builtin_bytes_encode_utf8", &["text"]),
                "utf8Decode" => ("builtin_bytes_decode_utf8", &["bytes"]),
                "hexEncode" => ("builtin_bytes_to_hex", &["bytes"]),
                "hexDecode" => ("builtin_bytes_from_hex", &["text"]),
                "base64Encode" => ("builtin_bytes_to_base64", &["bytes"]),
                "base64Decode" => ("builtin_bytes_from_base64", &["text"]),
                _ => return Err(decline("a boxed boundary Encoding static call")),
            };
            let args = boxed_boundary_args(args, params, ctx.src)?;
            let mut rendered = Vec::with_capacity(args.len());
            for arg in args {
                rendered.push(emit_boxed_boundary_expr(arg, ctx, scope)?);
            }
            return Ok(format!(
                "{leaf}({}, {})?",
                rendered.join(", "),
                emit_span(span)
            ));
        }
        if namespace == "Codec" && scope.iter().all(|local| local.name != namespace) {
            if member == "zstdCompress" {
                let (bytes, level) = boxed_boundary_zstd_args(args, ctx.src)?;
                let bytes = emit_boxed_boundary_expr(bytes, ctx, scope)?;
                let level = match level {
                    Some(expr) => emit_boxed_boundary_expr(expr, ctx, scope)?,
                    None => "Value::Int(3)".to_string(),
                };
                return Ok(format!(
                    "builtin_codec_zstd_compress({bytes}, {level}, {})?",
                    emit_span(span)
                ));
            }
            let (leaf, params): (&str, &[&str]) = match member {
                "gzipCompress" => ("builtin_codec_gzip_compress", &["bytes"]),
                "gzipDecompress" => ("builtin_codec_gzip_decompress", &["bytes"]),
                "deflateCompress" => ("builtin_codec_deflate_compress", &["bytes"]),
                "deflateFixedCompress" => ("builtin_codec_deflate_fixed_compress", &["bytes"]),
                "zlibFixedCompress" => ("builtin_codec_zlib_fixed_compress", &["bytes"]),
                "reedSolomon255223Protect" => {
                    ("builtin_codec_reed_solomon_255_223_protect", &["bytes"])
                }
                "deflateDecompress" => ("builtin_codec_deflate_decompress", &["bytes"]),
                "zstdDecompress" => ("builtin_codec_zstd_decompress", &["bytes"]),
                _ => return Err(decline("a boxed boundary Codec static call")),
            };
            let args = boxed_boundary_args(args, params, ctx.src)?;
            let mut rendered = Vec::with_capacity(args.len());
            for arg in args {
                rendered.push(emit_boxed_boundary_expr(arg, ctx, scope)?);
            }
            return Ok(format!(
                "{leaf}({}, {})?",
                rendered.join(", "),
                emit_span(span)
            ));
        }
    }

    if is_boxed_boundary_receiver_method(member) {
        emit_boxed_boundary_method_call(member, object, args, ctx, scope, callee.span, span)
    } else {
        Err(decline("a boxed boundary member call"))
    }
}

pub(super) fn is_boxed_boundary_receiver_method(member: &str) -> bool {
    matches!(
        member,
        // Bytes instance methods.
        "decodeUtf8"
            | "toHex"
            | "toBase64"
            | "length"
            | "isEmpty"
            | "toArray"
            | "get"
            | "slice"
            // String read-only methods.
            | "scalars"
            | "startsWith"
            | "endsWith"
            | "contains"
            | "indexOf"
            | "lastIndexOf"
            | "trim"
            | "trimStart"
            | "trimEnd"
            | "byteLength"
            | "codePointAt"
            | "split"
            | "replace"
            // Array read-only methods.
            | "join"
            | "sorted"
            // Map read-only methods.
            | "getOr"
            | "containsKey"
            // Set read-only methods.
            | "union"
            | "intersection"
            | "difference"
            // BigInt / Decimal methods.
            | "toString"
            | "toInt"
            | "div"
            | "mod"
            | "scale"
            | "round"
            // Option eager bridge.
            | "okOr"
    )
}

pub(super) fn emit_boxed_boundary_method_call(
    member: &str,
    object: &Expr,
    args: &[ast::CallArg],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    member_span: Span,
    call_span: Span,
) -> Result<String, EmitError> {
    let recv = emit_boxed_boundary_expr(object, ctx, scope)?;
    emit_boxed_boundary_method_dispatch(member, &recv, args, ctx, scope, member_span, call_span)
}

pub(super) fn emit_boxed_boundary_method_dispatch(
    member: &str,
    recv: &str,
    args: &[ast::CallArg],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    member_span: Span,
    call_span: Span,
) -> Result<String, EmitError> {
    let mut saw_named = false;
    let mut positional = Vec::new();
    let mut named = Vec::new();
    for arg in args {
        match arg {
            ast::CallArg::Positional(expr) if saw_named => {
                return Err(decline("a boxed boundary method argument shape"));
            }
            ast::CallArg::Positional(expr) => {
                positional.push(emit_boxed_boundary_expr(expr, ctx, scope)?);
            }
            ast::CallArg::Named { name, value } => {
                saw_named = true;
                let name = text(ctx.src, name.span);
                named.push(format!(
                    "({name:?}.to_string(), {})",
                    emit_boxed_boundary_expr(value, ctx, scope)?
                ));
            }
            ast::CallArg::Spread(_) => return Err(decline("a boxed boundary method spread")),
        }
    }

    let positional_args = positional.join(", ");
    let member_span = emit_span(member_span);
    let call_span = emit_span(call_span);
    let some_arm = if named.is_empty() {
        format!("call_value(__f, vec![{positional_args}], cx.clone(), {call_span}).await?")
    } else {
        format!(
            "call_value_named(__f, vec![{positional_args}], vec![{}], cx.clone(), {call_span}).await?",
            named.join(", ")
        )
    };
    let none_arm = if named.is_empty() {
        format!(
            "check_member_method(&__recv, {member:?}, {member_span})?; call_method(__recv, {member:?}, vec![{positional_args}], {member_span}, {call_span})?"
        )
    } else {
        format!(
            "check_member_method(&__recv, {member:?}, {member_span})?; return Err(fault(codes::GUARD_ARITY, \"named arguments require a first-class method\", {call_span}));"
        )
    };
    Ok(format!(
        "{{ let __recv = {recv}; \
         match member_value(&__recv, {member:?}, {member_span})? {{ \
         Some(__f) => {some_arm}, \
         None => {{ {none_arm} }}, \
         }} }}"
    ))
}

pub(super) fn emit_boxed_boundary_leaf_call(
    leaf: &str,
    args: &[ast::CallArg],
    params: &[&str],
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
    span: Span,
) -> Result<String, EmitError> {
    let args = boxed_boundary_args(args, params, ctx.src)?;
    let mut rendered = Vec::with_capacity(args.len());
    for arg in args {
        rendered.push(emit_boxed_boundary_expr(arg, ctx, scope)?);
    }
    let call_args = if rendered.is_empty() {
        emit_span(span)
    } else {
        format!("{}, {}", rendered.join(", "), emit_span(span))
    };
    Ok(format!("{leaf}({call_args})?"))
}

pub(super) fn boxed_boundary_args<'a>(
    args: &'a [ast::CallArg],
    params: &[&str],
    src: &LoweredText,
) -> Result<Vec<&'a Expr>, EmitError> {
    if args.len() != params.len() {
        return Err(decline("a boxed boundary argument count"));
    }
    let mut slots: Vec<Option<&Expr>> = vec![None; params.len()];
    let mut next_positional = 0usize;
    for arg in args {
        match arg {
            ast::CallArg::Positional(expr) => {
                if next_positional >= params.len() || slots[next_positional].is_some() {
                    return Err(decline("a boxed boundary argument shape"));
                }
                slots[next_positional] = Some(expr);
                next_positional += 1;
            }
            ast::CallArg::Named { name, value } => {
                let nm = text(src, name.span);
                let Some(index) = params.iter().position(|param| *param == nm) else {
                    return Err(decline("a boxed boundary argument name"));
                };
                if slots[index].is_some() {
                    return Err(decline("a boxed boundary duplicate argument"));
                }
                slots[index] = Some(value);
            }
            ast::CallArg::Spread(_) => return Err(decline("a boxed boundary spread argument")),
        }
    }
    slots
        .into_iter()
        .map(|slot| slot.ok_or_else(|| decline("a boxed boundary missing argument")))
        .collect()
}

pub(super) fn boxed_boundary_zstd_args<'a>(
    args: &'a [ast::CallArg],
    src: &LoweredText,
) -> Result<(&'a Expr, Option<&'a Expr>), EmitError> {
    if !(1..=2).contains(&args.len()) {
        return Err(decline("a boxed boundary argument count"));
    }
    let params = ["bytes", "level"];
    let mut slots: [Option<&Expr>; 2] = [None, None];
    let mut next_positional = 0usize;
    for arg in args {
        match arg {
            ast::CallArg::Positional(expr) => {
                if next_positional >= params.len() || slots[next_positional].is_some() {
                    return Err(decline("a boxed boundary argument shape"));
                }
                slots[next_positional] = Some(expr);
                next_positional += 1;
            }
            ast::CallArg::Named { name, value } => {
                let nm = text(src, name.span);
                let Some(index) = params.iter().position(|param| *param == nm) else {
                    return Err(decline("a boxed boundary argument name"));
                };
                if slots[index].is_some() {
                    return Err(decline("a boxed boundary duplicate argument"));
                }
                slots[index] = Some(value);
            }
            ast::CallArg::Spread(_) => return Err(decline("a boxed boundary spread argument")),
        }
    }
    let Some(bytes) = slots[0] else {
        return Err(decline("a boxed boundary missing argument"));
    };
    Ok((bytes, slots[1]))
}

/// Lower the INITIALIZER of a scalar-array boundary local to a boxed
/// `Value::Array`. Only a direct array LITERAL of native-scalar elements (each of
/// the declared element type) is supported this slice — the array is built inline
/// as `Value::array(vec![<boxed e1>, …])`, exactly the boxed backend's array
/// lowering, so the runtime array is identical. Any other initializer (a call, a
/// spread, a non-scalar element) declines → boxed.
pub(super) fn emit_boxed_scalar_array(
    value: &Expr,
    elem: NativeTy,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<String, EmitError> {
    let ExprKind::Array(elements) = &value.kind else {
        return Err(decline("a non-literal array boundary initializer").at(value.span));
    };
    let mut boxed = Vec::with_capacity(elements.len());
    for el in elements {
        let ast::ArrayElement::Expr(e) = el else {
            return Err(decline("a spread in an array boundary initializer").at(value.span));
        };
        let low = emit_expr(e, ctx, scope)?;
        if low.ty != elem {
            return Err(
                decline("an array element whose type differs from the declared element").at(e.span),
            );
        }
        // Box the native scalar element into its `Value` (the array is boxed).
        boxed.push(elem.box_expr(&low.rs));
    }
    Ok(format!("Value::array(vec![{}])", boxed.join(", ")))
}

/// Lower a direct scalar array literal while inferring its element scalar from
/// the literal elements. Used by statement-position native `for` loops where the
/// iterable has no explicit `Array<T>` boundary annotation.
pub(super) fn emit_boxed_scalar_array_inferred(
    value: &Expr,
    ctx: &Ctx<'_>,
    scope: &[NativeLocal],
) -> Result<(String, NativeTy), EmitError> {
    let ExprKind::Array(elements) = &value.kind else {
        return Err(decline("a non-literal native `for` array").at(value.span));
    };
    let mut elem_ty = None;
    let mut boxed = Vec::with_capacity(elements.len());
    for el in elements {
        let ast::ArrayElement::Expr(e) = el else {
            return Err(decline("a spread in a native `for` array").at(value.span));
        };
        let low = emit_expr(e, ctx, scope)?;
        match elem_ty {
            Some(ty) if ty != low.ty => {
                return Err(decline("mixed element types in a native `for` array").at(e.span));
            }
            Some(_) => {}
            None => elem_ty = Some(low.ty),
        }
        boxed.push(low.ty.box_expr(&low.rs));
    }
    let elem = elem_ty.ok_or_else(|| decline("an empty native `for` array").at(value.span))?;
    Ok((format!("Value::array(vec![{}])", boxed.join(", ")), elem))
}
