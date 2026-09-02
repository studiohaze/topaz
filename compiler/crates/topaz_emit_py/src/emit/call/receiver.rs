use crate::*;

pub(super) fn emit_receiver_readonly_builtin_call(
    object: &Expr,
    method: &str,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<Option<String>, PyEmitError> {
    if template_value(object, ctx) {
        return Err(PyEmitError::unsupported("member call").at(span));
    }
    if string_value(object, ctx)
        && let Some(spec) = string_receiver_builtin(method)
    {
        let bound = bind_fixed_static_call_args(args, spec.params, &[], span, ctx)?;
        let recv = emit_expr(object, ctx)?;
        return Ok(Some(render_bound_receiver_static_call(
            recv,
            &bound,
            |recv, slots| spec.render(recv, slots, span),
        )));
    }
    if receiver_is_array_value(object, ctx)
        && let Some(spec) = array_receiver_builtin(method)
    {
        let bound = bind_fixed_static_call_args(args, spec.params, &[], span, ctx)?;
        let recv = emit_expr(object, ctx)?;
        return Ok(Some(render_bound_receiver_static_call(
            recv,
            &bound,
            |recv, slots| spec.render(recv, slots, span),
        )));
    }
    match method {
        "length" if receiver_is_byte_buffer_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_byte_buffer_length({recv}, {})", py_span(span)),
            )))
        }
        "get" if receiver_is_byte_buffer_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["index"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    format!(
                        "tpz_byte_buffer_get({recv}, {}, {})",
                        slots[0],
                        py_span(span)
                    )
                },
            )))
        }
        "set" if receiver_is_byte_buffer_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["index", "value"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    format!(
                        "tpz_byte_buffer_set({recv}, {}, {}, {})",
                        slots[0],
                        slots[1],
                        py_span(span)
                    )
                },
            )))
        }
        "fill" if receiver_is_byte_buffer_value(object, ctx) => {
            let bound =
                bind_fixed_static_call_args(args, &["start", "length", "value"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    format!(
                        "tpz_byte_buffer_fill({recv}, {}, {}, {}, {})",
                        slots[0],
                        slots[1],
                        slots[2],
                        py_span(span)
                    )
                },
            )))
        }
        "copy" if receiver_is_byte_buffer_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(
                args,
                &["source", "sourceStart", "targetStart", "length"],
                &[],
                span,
                ctx,
            )?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    format!(
                        "tpz_byte_buffer_copy({recv}, {}, {}, {}, {}, {})",
                        slots[0],
                        slots[1],
                        slots[2],
                        slots[3],
                        py_span(span)
                    )
                },
            )))
        }
        "toBytes" if receiver_is_byte_buffer_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_byte_buffer_to_bytes({recv}, {})", py_span(span)),
            )))
        }
        "value" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_newtype_unwrap({recv}, {})", py_span(span)),
            )))
        }
        "split" => {
            let bound = bind_fixed_static_call_args(args, &["sep"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| format!("tpz_split({recv}, {}, {})", slots[0], py_span(span)),
            )))
        }
        "codePointAt" => {
            let bound = bind_fixed_static_call_args(args, &["i"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    format!(
                        "tpz_string_code_point_at({recv}, {}, {})",
                        slots[0],
                        py_span(span)
                    )
                },
            )))
        }
        "byteLength" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_string_byte_length({recv}, {})", py_span(span)),
            )))
        }
        "trim" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_string_trim({recv}, {})", py_span(span)),
            )))
        }
        "scalars" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("list({recv})"),
            )))
        }
        "join" => {
            let bound = bind_fixed_static_call_args(args, &["sep"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| format!("tpz_array_join({recv}, {}, {})", slots[0], py_span(span)),
            )))
        }
        "get" if receiver_is_array_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["i"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
            )))
        }
        "get" if receiver_is_map_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["k"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
            )))
        }
        "get" if receiver_is_json_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["key"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
            )))
        }
        "get" if receiver_is_bytes_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["index"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
            )))
        }
        "get" => {
            let bound = bind_fixed_static_call_args(args, &["key"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
            )))
        }
        "getOr" => {
            let bound = bind_fixed_static_call_args(args, &["k", "default"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    format!(
                        "tpz_map_get_or({recv}, {}, {}, {})",
                        slots[0],
                        slots[1],
                        py_span(span)
                    )
                },
            )))
        }
        "containsKey" => {
            let bound = bind_fixed_static_call_args(args, &["k"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    format!(
                        "tpz_map_contains_key({recv}, {}, {})",
                        slots[0],
                        py_span(span)
                    )
                },
            )))
        }
        "contains" => {
            let bound = bind_fixed_static_call_args(args, &["x"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| format!("tpz_set_contains({recv}, {}, {})", slots[0], py_span(span)),
            )))
        }
        "union" => {
            let bound = bind_fixed_static_call_args(args, &["other"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| format!("tpz_set_union({recv}, {}, {})", slots[0], py_span(span)),
            )))
        }
        "intersection" => {
            let bound = bind_fixed_static_call_args(args, &["other"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    format!(
                        "tpz_set_intersection({recv}, {}, {})",
                        slots[0],
                        py_span(span)
                    )
                },
            )))
        }
        "difference" => {
            let bound = bind_fixed_static_call_args(args, &["other"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    format!(
                        "tpz_set_difference({recv}, {}, {})",
                        slots[0],
                        py_span(span)
                    )
                },
            )))
        }
        "decodeUtf8" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_bytes_decode_utf8({recv}, {})", py_span(span)),
            )))
        }
        "toHex" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_bytes_to_hex({recv}, {})", py_span(span)),
            )))
        }
        "toBase64" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_bytes_to_base64({recv}, {})", py_span(span)),
            )))
        }
        "isEmpty" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_is_empty({recv}, {})", py_span(span)),
            )))
        }
        "slice" if receiver_is_bytes_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["start", "end"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    format!(
                        "tpz_bytes_slice({recv}, {}, {}, {})",
                        slots[0],
                        slots[1],
                        py_span(span)
                    )
                },
            )))
        }
        "toArray" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_to_array({recv}, {})", py_span(span)),
            )))
        }
        "kind" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_json_kind({recv}, {})", py_span(span)),
            )))
        }
        "isNull" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_json_is_null({recv}, {})", py_span(span)),
            )))
        }
        "asString" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_json_as_string({recv}, {})", py_span(span)),
            )))
        }
        "asBool" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_json_as_bool({recv}, {})", py_span(span)),
            )))
        }
        "asInt" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_json_as_int({recv}, {})", py_span(span)),
            )))
        }
        "numberText" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_json_number_text({recv}, {})", py_span(span)),
            )))
        }
        "asArray" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_json_as_array({recv}, {})", py_span(span)),
            )))
        }
        "keys" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_json_keys({recv}, {})", py_span(span)),
            )))
        }
        "values" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_json_values({recv}, {})", py_span(span)),
            )))
        }
        "isMatch" => {
            let bound = bind_fixed_static_call_args(args, &["text"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    format!(
                        "tpz_regex_is_match({recv}, {}, {})",
                        slots[0],
                        py_span(span)
                    )
                },
            )))
        }
        "replaceAll" => {
            let bound =
                bind_fixed_static_call_args(args, &["text", "replacement"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    format!(
                        "tpz_regex_replace_all({recv}, {}, {}, {})",
                        slots[0],
                        slots[1],
                        py_span(span)
                    )
                },
            )))
        }
        "path" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_url_path({recv}, {})", py_span(span)),
            )))
        }
        "toString" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_url_to_string({recv}, {})", py_span(span)),
            )))
        }
        "at" => {
            let bound = bind_fixed_static_call_args(args, &["index"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    format!(
                        "tpz_json_at({recv}, {}, {}, {})",
                        slots[0],
                        py_span(span),
                        py_span(span)
                    )
                },
            )))
        }
        "length" => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_length({recv}, {})", py_span(span)),
            )))
        }
        _ => Ok(None),
    }
}

pub(super) fn emit_receiver_callback_builtin_call(
    object: &Expr,
    method: &str,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<Option<String>, PyEmitError> {
    match method {
        "map" => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            if receiver_is_array_value(object, ctx) {
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    move |recv, slots| {
                        render_array_map_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            cooperative,
                            cooperative_callback,
                        )
                    },
                )));
            }
            if receiver_is_option_value(object, ctx) {
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    move |recv, slots| {
                        render_option_map_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            cooperative,
                            cooperative_callback,
                        )
                    },
                )));
            }
            if receiver_is_result_value(object, ctx) {
                let cooperative_callback = bound.slot_is_cooperative_callback(0);
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    |recv, slots| {
                        render_result_map_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            cooperative,
                            cooperative_callback,
                        )
                    },
                )));
            }
            Ok(None)
        }
        "filter" => {
            if receiver_is_map_value(object, ctx) {
                let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 2)], span, ctx)?;
                let recv = emit_expr(object, ctx)?;
                let cooperative = ctx.cooperative_yields;
                let cooperative_callback = bound.slot_is_cooperative_callback(0);
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    |recv, slots| {
                        render_map_filter_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            cooperative,
                            cooperative_callback,
                        )
                    },
                )));
            }
            if receiver_is_array_value(object, ctx) {
                let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
                let recv = emit_expr(object, ctx)?;
                let cooperative = ctx.cooperative_yields;
                let cooperative_callback = bound.slot_is_cooperative_callback(0);
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    move |recv, slots| {
                        render_array_filter_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            cooperative,
                            cooperative_callback,
                        )
                    },
                )));
            }
            Ok(None)
        }
        "reduce" if receiver_is_array_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["initial", "f"], &[(1, 2)], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(1);
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                move |recv, slots| {
                    render_array_reduce_call_with_callback(
                        recv,
                        &slots[0],
                        &slots[1],
                        span,
                        cooperative,
                        cooperative_callback,
                    )
                },
            )))
        }
        "sorted" if receiver_is_array_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_array_sorted({recv}, {})", py_span(span)),
            )))
        }
        "sort" if receiver_is_array_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &[], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_array_sort({recv}, {})", py_span(span)),
            )))
        }
        "sortedBy" if receiver_is_array_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    render_array_sorted_by_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        cooperative,
                        cooperative_callback,
                    )
                },
            )))
        }
        "sortBy" if receiver_is_array_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    render_array_sort_by_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        cooperative,
                        cooperative_callback,
                    )
                },
            )))
        }
        "retain" if receiver_is_array_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    render_array_retain_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        cooperative,
                        cooperative_callback,
                    )
                },
            )))
        }
        "flatMap" => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            if receiver_is_option_value(object, ctx) {
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    |recv, slots| {
                        render_option_flat_map_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            cooperative,
                            cooperative_callback,
                        )
                    },
                )));
            }
            if receiver_is_result_value(object, ctx) {
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    |recv, slots| {
                        render_result_flat_map_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            cooperative,
                            cooperative_callback,
                        )
                    },
                )));
            }
            Ok(None)
        }
        "okOrElse" if receiver_is_option_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 0)], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    render_option_ok_or_else_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        cooperative,
                        cooperative_callback,
                    )
                },
            )))
        }
        "okOr" if receiver_is_option_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["error"], &[], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| format!("tpz_option_ok_or({recv}, {}, {})", slots[0], py_span(span)),
            )))
        }
        "mapValues" if receiver_is_map_value(object, ctx) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    render_map_map_values_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        cooperative,
                        cooperative_callback,
                    )
                },
            )))
        }
        "update" if receiver_is_map_value(object, ctx) => {
            let bound =
                bind_fixed_static_call_args(args, &["k", "initial", "f"], &[(2, 1)], span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(2);
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    render_map_update_call_with_callback(
                        recv,
                        &slots[0],
                        &slots[1],
                        &slots[2],
                        span,
                        cooperative,
                        cooperative_callback,
                    )
                },
            )))
        }
        _ => Ok(None),
    }
}

pub(super) fn emit_pipe_receiver_callback_builtin_call(
    lhs: &Expr,
    object: &Expr,
    method: &str,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<Option<String>, PyEmitError> {
    if string_value(object, ctx)
        && let Some(spec) = string_receiver_builtin(method)
    {
        let bound =
            bind_pipe_static_call_args(args, spec.params, &[], lhs, "__tpz_piped", span, ctx)?;
        let recv = emit_expr(object, ctx)?;
        return Ok(Some(render_bound_receiver_static_call(
            recv,
            &bound,
            |recv, slots| spec.render(recv, slots, span),
        )));
    }
    if receiver_is_array_value(object, ctx)
        && let Some(spec) = array_receiver_builtin(method)
    {
        let bound =
            bind_pipe_static_call_args(args, spec.params, &[], lhs, "__tpz_piped", span, ctx)?;
        let recv = emit_expr(object, ctx)?;
        return Ok(Some(render_bound_receiver_static_call(
            recv,
            &bound,
            |recv, slots| spec.render(recv, slots, span),
        )));
    }
    match method {
        "map" => {
            let recv = emit_expr(object, ctx)?;
            if receiver_is_array_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                let bound = bind_pipe_static_call_args(
                    args,
                    &["f"],
                    &[(0, 1)],
                    lhs,
                    "__tpz_piped",
                    span,
                    ctx,
                )?;
                let cooperative_callback = bound.slot_is_cooperative_callback(0);
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    move |recv, slots| {
                        render_array_map_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            cooperative,
                            cooperative_callback,
                        )
                    },
                )));
            }
            if receiver_is_option_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                let bound = bind_pipe_static_call_args(
                    args,
                    &["f"],
                    &[(0, 1)],
                    lhs,
                    "__tpz_piped",
                    span,
                    ctx,
                )?;
                let cooperative_callback = bound.slot_is_cooperative_callback(0);
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    move |recv, slots| {
                        render_option_map_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            cooperative,
                            cooperative_callback,
                        )
                    },
                )));
            }
            if receiver_is_result_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                let bound = bind_pipe_static_call_args(
                    args,
                    &["f"],
                    &[(0, 1)],
                    lhs,
                    "__tpz_piped",
                    span,
                    ctx,
                )?;
                let cooperative_callback = bound.slot_is_cooperative_callback(0);
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    move |recv, slots| {
                        render_result_map_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            cooperative,
                            cooperative_callback,
                        )
                    },
                )));
            }
            Ok(None)
        }
        "filter" => {
            if receiver_is_map_value(object, ctx) {
                let bound = bind_pipe_static_call_args(
                    args,
                    &["f"],
                    &[(0, 2)],
                    lhs,
                    "__tpz_piped",
                    span,
                    ctx,
                )?;
                let recv = emit_expr(object, ctx)?;
                let cooperative = ctx.cooperative_yields;
                let cooperative_callback = bound.slot_is_cooperative_callback(0);
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    |recv, slots| {
                        render_map_filter_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            cooperative,
                            cooperative_callback,
                        )
                    },
                )));
            }
            if receiver_is_array_value(object, ctx) {
                let cooperative = ctx.cooperative_yields;
                let bound = bind_pipe_static_call_args(
                    args,
                    &["f"],
                    &[(0, 1)],
                    lhs,
                    "__tpz_piped",
                    span,
                    ctx,
                )?;
                let recv = emit_expr(object, ctx)?;
                let cooperative_callback = bound.slot_is_cooperative_callback(0);
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    move |recv, slots| {
                        render_array_filter_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            cooperative,
                            cooperative_callback,
                        )
                    },
                )));
            }
            Ok(None)
        }
        "reduce" if receiver_is_array_value(object, ctx) => {
            let cooperative = ctx.cooperative_yields;
            let bound = bind_pipe_static_call_args(
                args,
                &["initial", "f"],
                &[(1, 2)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let recv = emit_expr(object, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(1);
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                move |recv, slots| {
                    render_array_reduce_call_with_callback(
                        recv,
                        &slots[0],
                        &slots[1],
                        span,
                        cooperative,
                        cooperative_callback,
                    )
                },
            )))
        }
        "sorted" if receiver_is_array_value(object, ctx) => {
            let bound = bind_pipe_static_call_args(args, &[], &[], lhs, "__tpz_piped", span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_array_sorted({recv}, {})", py_span(span)),
            )))
        }
        "sort" if receiver_is_array_value(object, ctx) => {
            let bound = bind_pipe_static_call_args(args, &[], &[], lhs, "__tpz_piped", span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, _| format!("tpz_array_sort({recv}, {})", py_span(span)),
            )))
        }
        "sortedBy" if receiver_is_array_value(object, ctx) => {
            let bound =
                bind_pipe_static_call_args(args, &["f"], &[(0, 1)], lhs, "__tpz_piped", span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    render_array_sorted_by_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        cooperative,
                        cooperative_callback,
                    )
                },
            )))
        }
        "flatMap" => {
            let recv = emit_expr(object, ctx)?;
            if receiver_is_option_value(object, ctx) {
                let bound = bind_pipe_static_call_args(
                    args,
                    &["f"],
                    &[(0, 1)],
                    lhs,
                    "__tpz_piped",
                    span,
                    ctx,
                )?;
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    |recv, slots| {
                        render_option_flat_map_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            ctx.cooperative_yields,
                            bound.slot_is_cooperative_callback(0),
                        )
                    },
                )));
            }
            if receiver_is_result_value(object, ctx) {
                let bound = bind_pipe_static_call_args(
                    args,
                    &["f"],
                    &[(0, 1)],
                    lhs,
                    "__tpz_piped",
                    span,
                    ctx,
                )?;
                return Ok(Some(render_bound_receiver_static_call(
                    recv,
                    &bound,
                    |recv, slots| {
                        render_result_flat_map_call_with_callback(
                            recv,
                            &slots[0],
                            span,
                            ctx.cooperative_yields,
                            bound.slot_is_cooperative_callback(0),
                        )
                    },
                )));
            }
            Ok(None)
        }
        "okOrElse" if receiver_is_option_value(object, ctx) => {
            let bound =
                bind_pipe_static_call_args(args, &["f"], &[(0, 0)], lhs, "__tpz_piped", span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    render_option_ok_or_else_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        cooperative,
                        cooperative_callback,
                    )
                },
            )))
        }
        "okOr" if receiver_is_option_value(object, ctx) => {
            let bound =
                bind_pipe_static_call_args(args, &["error"], &[], lhs, "__tpz_piped", span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| format!("tpz_option_ok_or({recv}, {}, {})", slots[0], py_span(span)),
            )))
        }
        "mapValues" if receiver_is_map_value(object, ctx) => {
            let bound =
                bind_pipe_static_call_args(args, &["f"], &[(0, 1)], lhs, "__tpz_piped", span, ctx)?;
            let recv = emit_expr(object, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    render_map_map_values_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        cooperative,
                        cooperative_callback,
                    )
                },
            )))
        }
        "update" if receiver_is_map_value(object, ctx) => {
            let bound = bind_pipe_static_call_args(
                args,
                &["k", "initial", "f"],
                &[(2, 1)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let recv = emit_expr(object, ctx)?;
            let cooperative = ctx.cooperative_yields;
            let cooperative_callback = bound.slot_is_cooperative_callback(2);
            Ok(Some(render_bound_receiver_static_call(
                recv,
                &bound,
                |recv, slots| {
                    render_map_update_call_with_callback(
                        recv,
                        &slots[0],
                        &slots[1],
                        &slots[2],
                        span,
                        cooperative,
                        cooperative_callback,
                    )
                },
            )))
        }
        _ => Ok(None),
    }
}

pub(super) fn emit_pipe_optional_receiver_callback_builtin_call(
    lhs: &Expr,
    object: &Expr,
    method: &str,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<Option<String>, PyEmitError> {
    let Some(shape) = optional_receiver_inner_shape(object, ctx) else {
        return Ok(None);
    };

    if shape == ReceiverShape::String
        && let Some(spec) = string_receiver_builtin(method)
    {
        return Ok(Some(render_optional_receiver_static_call(
            object,
            bind_fixed_pipe_static_call_args(
                args,
                spec.params,
                &[],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?,
            ctx,
            |recv, slots| spec.render(recv, slots, span),
        )?));
    }
    if shape == ReceiverShape::Array
        && let Some(spec) = array_receiver_builtin(method)
    {
        let bound = bind_fixed_pipe_static_call_args(
            args,
            spec.params,
            &[],
            lhs,
            "__tpz_piped",
            span,
            ctx,
        )?;
        if spec.returns_unit {
            return Ok(Some(render_optional_receiver_unit_call(
                object,
                bound,
                ctx,
                |recv, slots| spec.render(recv, slots, span),
            )?));
        }
        return Ok(Some(render_optional_receiver_static_call(
            object,
            bound,
            ctx,
            |recv, slots| spec.render(recv, slots, span),
        )?));
    }

    let rendered = match (method, shape) {
        ("map", ReceiverShape::Array) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["f"],
                &[(0, 1)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_static_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_array_map_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("map", ReceiverShape::Option) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["f"],
                &[(0, 1)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_static_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_option_map_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("map", ReceiverShape::Result) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["f"],
                &[(0, 1)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_static_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_result_map_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("filter", ReceiverShape::Array) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["f"],
                &[(0, 1)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_static_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_array_filter_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("filter", ReceiverShape::Map) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["f"],
                &[(0, 2)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_static_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_map_filter_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("reduce", ReceiverShape::Array) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["initial", "f"],
                &[(1, 2)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(1);
            Some(render_optional_receiver_static_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_array_reduce_call_with_callback(
                        recv,
                        &slots[0],
                        &slots[1],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("sorted", ReceiverShape::Array) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_pipe_static_call_args(args, &[], &[], lhs, "__tpz_piped", span, ctx)?,
            ctx,
            |recv, _| format!("tpz_array_sorted({recv}, {})", py_span(span)),
        )?),
        ("sort", ReceiverShape::Array) => Some(render_optional_receiver_unit_call(
            object,
            bind_fixed_pipe_static_call_args(args, &[], &[], lhs, "__tpz_piped", span, ctx)?,
            ctx,
            |recv, _| format!("tpz_array_sort({recv}, {})", py_span(span)),
        )?),
        ("sortedBy", ReceiverShape::Array) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["f"],
                &[(0, 1)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_static_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_array_sorted_by_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("sortBy", ReceiverShape::Array) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["f"],
                &[(0, 1)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_unit_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_array_sort_by_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("retain", ReceiverShape::Array) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["f"],
                &[(0, 1)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_unit_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_array_retain_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("flatMap", ReceiverShape::Option) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["f"],
                &[(0, 1)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_static_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_option_flat_map_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("flatMap", ReceiverShape::Result) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["f"],
                &[(0, 1)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_static_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_result_flat_map_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("okOrElse", ReceiverShape::Option) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["f"],
                &[(0, 0)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_static_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_option_ok_or_else_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("okOr", ReceiverShape::Option) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_pipe_static_call_args(args, &["error"], &[], lhs, "__tpz_piped", span, ctx)?,
            ctx,
            |recv, slots| format!("tpz_option_ok_or({recv}, {}, {})", slots[0], py_span(span)),
        )?),
        ("mapValues", ReceiverShape::Map) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["f"],
                &[(0, 1)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_static_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_map_map_values_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("update", ReceiverShape::Map) => {
            let bound = bind_fixed_pipe_static_call_args(
                args,
                &["k", "initial", "f"],
                &[(2, 1)],
                lhs,
                "__tpz_piped",
                span,
                ctx,
            )?;
            let cooperative_callback = bound.slot_is_cooperative_callback(2);
            Some(render_optional_receiver_unit_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_map_update_call_with_callback(
                        recv,
                        &slots[0],
                        &slots[1],
                        &slots[2],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        _ => None,
    };
    Ok(rendered)
}

pub(super) fn emit_optional_receiver_readonly_builtin_call(
    object: &Expr,
    method: &str,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<Option<String>, PyEmitError> {
    let Some(shape) = optional_receiver_inner_shape(object, ctx) else {
        return Ok(None);
    };

    if shape == ReceiverShape::String
        && let Some(spec) = string_receiver_builtin(method)
    {
        return Ok(Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, spec.params, &[], span, ctx)?,
            ctx,
            |recv, slots| spec.render(recv, slots, span),
        )?));
    }
    if shape == ReceiverShape::Array
        && let Some(spec) = array_receiver_builtin(method)
    {
        let bound = bind_fixed_static_call_args(args, spec.params, &[], span, ctx)?;
        if spec.returns_unit {
            return Ok(Some(render_optional_receiver_unit_call(
                object,
                bound,
                ctx,
                |recv, slots| spec.render(recv, slots, span),
            )?));
        }
        return Ok(Some(render_optional_receiver_static_call(
            object,
            bound,
            ctx,
            |recv, slots| spec.render(recv, slots, span),
        )?));
    }

    let rendered = match (method, shape) {
        ("split", ReceiverShape::String) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &["sep"], &[], span, ctx)?,
            ctx,
            |recv, slots| format!("tpz_string_split({recv}, {}, {})", slots[0], py_span(span)),
        )?),
        ("codePointAt", ReceiverShape::String) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &["i"], &[], span, ctx)?,
            ctx,
            |recv, slots| {
                format!(
                    "tpz_string_code_point_at({recv}, {}, {})",
                    slots[0],
                    py_span(span)
                )
            },
        )?),
        ("byteLength", ReceiverShape::String) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("tpz_string_byte_length({recv}, {})", py_span(span)),
        )?),
        ("scalars", ReceiverShape::String) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("list({recv})"),
        )?),
        ("get", ReceiverShape::Array) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &["i"], &[], span, ctx)?,
            ctx,
            |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
        )?),
        ("get", ReceiverShape::Map) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &["k"], &[], span, ctx)?,
            ctx,
            |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
        )?),
        ("get", ReceiverShape::Bytes) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &["index"], &[], span, ctx)?,
            ctx,
            |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
        )?),
        ("get", ReceiverShape::Json) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &["key"], &[], span, ctx)?,
            ctx,
            |recv, slots| format!("tpz_get({recv}, {}, {})", slots[0], py_span(span)),
        )?),
        ("getOr", ReceiverShape::Map) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &["k", "default"], &[], span, ctx)?,
            ctx,
            |recv, slots| {
                format!(
                    "tpz_map_get_or({recv}, {}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            },
        )?),
        ("containsKey", ReceiverShape::Map) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &["k"], &[], span, ctx)?,
            ctx,
            |recv, slots| {
                format!(
                    "tpz_map_contains_key({recv}, {}, {})",
                    slots[0],
                    py_span(span)
                )
            },
        )?),
        ("decodeUtf8", ReceiverShape::Bytes) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("tpz_bytes_decode_utf8({recv}, {})", py_span(span)),
        )?),
        ("toHex", ReceiverShape::Bytes) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("tpz_bytes_to_hex({recv}, {})", py_span(span)),
        )?),
        ("toBase64", ReceiverShape::Bytes) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("tpz_bytes_to_base64({recv}, {})", py_span(span)),
        )?),
        ("isEmpty", ReceiverShape::Bytes | ReceiverShape::Map) => {
            Some(render_optional_receiver_static_call(
                object,
                bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
                ctx,
                |recv, _| format!("tpz_is_empty({recv}, {})", py_span(span)),
            )?)
        }
        ("slice", ReceiverShape::Bytes) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &["start", "end"], &[], span, ctx)?,
            ctx,
            |recv, slots| {
                format!(
                    "tpz_bytes_slice({recv}, {}, {}, {})",
                    slots[0],
                    slots[1],
                    py_span(span)
                )
            },
        )?),
        ("toArray", ReceiverShape::Bytes) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("tpz_to_array({recv}, {})", py_span(span)),
        )?),
        ("kind", ReceiverShape::Json) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("tpz_json_kind({recv}, {})", py_span(span)),
        )?),
        ("isNull", ReceiverShape::Json) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("tpz_json_is_null({recv}, {})", py_span(span)),
        )?),
        ("asString", ReceiverShape::Json) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("tpz_json_as_string({recv}, {})", py_span(span)),
        )?),
        ("asBool", ReceiverShape::Json) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("tpz_json_as_bool({recv}, {})", py_span(span)),
        )?),
        ("asInt", ReceiverShape::Json) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("tpz_json_as_int({recv}, {})", py_span(span)),
        )?),
        ("numberText", ReceiverShape::Json) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("tpz_json_number_text({recv}, {})", py_span(span)),
        )?),
        ("at", ReceiverShape::Json) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &["index"], &[], span, ctx)?,
            ctx,
            |recv, slots| {
                format!(
                    "tpz_json_at({recv}, {}, {}, {})",
                    slots[0],
                    py_span(span),
                    py_span(span)
                )
            },
        )?),
        ("length", ReceiverShape::Bytes | ReceiverShape::Json) => {
            Some(render_optional_receiver_static_call(
                object,
                bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
                ctx,
                |recv, _| format!("tpz_length({recv}, {})", py_span(span)),
            )?)
        }
        _ => None,
    };
    Ok(rendered)
}

pub(super) fn emit_optional_receiver_callback_builtin_call(
    object: &Expr,
    method: &str,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<Option<String>, PyEmitError> {
    let Some(shape) = optional_receiver_inner_shape(object, ctx) else {
        return Ok(None);
    };

    let rendered = match (method, shape) {
        ("map", ReceiverShape::Array) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_callback_call(
                object,
                bound,
                span,
                ctx,
                |recv, slots| {
                    render_array_map_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("map", ReceiverShape::Option) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_callback_call(
                object,
                bound,
                span,
                ctx,
                |recv, slots| {
                    render_option_map_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("map", ReceiverShape::Result) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_callback_call(
                object,
                bound,
                span,
                ctx,
                |recv, slots| {
                    render_result_map_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("filter", ReceiverShape::Array) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_callback_call(
                object,
                bound,
                span,
                ctx,
                |recv, slots| {
                    render_array_filter_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("filter", ReceiverShape::Map) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 2)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_callback_call(
                object,
                bound,
                span,
                ctx,
                |recv, slots| {
                    render_map_filter_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("reduce", ReceiverShape::Array) => {
            let bound = bind_fixed_static_call_args(args, &["initial", "f"], &[(1, 2)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(1);
            Some(render_optional_receiver_callback_call(
                object,
                bound,
                span,
                ctx,
                |recv, slots| {
                    render_array_reduce_call_with_callback(
                        recv,
                        &slots[0],
                        &slots[1],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("sorted", ReceiverShape::Array) => Some(render_optional_receiver_static_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("tpz_array_sorted({recv}, {})", py_span(span)),
        )?),
        ("sort", ReceiverShape::Array) => Some(render_optional_receiver_unit_call(
            object,
            bind_fixed_static_call_args(args, &[], &[], span, ctx)?,
            ctx,
            |recv, _| format!("tpz_array_sort({recv}, {})", py_span(span)),
        )?),
        ("sortedBy", ReceiverShape::Array) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_callback_call(
                object,
                bound,
                span,
                ctx,
                |recv, slots| {
                    render_array_sorted_by_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("sortBy", ReceiverShape::Array) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_unit_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_array_sort_by_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("retain", ReceiverShape::Array) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_unit_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_array_retain_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("flatMap", ReceiverShape::Option) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_callback_call(
                object,
                bound,
                span,
                ctx,
                |recv, slots| {
                    render_option_flat_map_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("flatMap", ReceiverShape::Result) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_callback_call(
                object,
                bound,
                span,
                ctx,
                |recv, slots| {
                    render_result_flat_map_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("okOrElse", ReceiverShape::Option) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 0)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_callback_call(
                object,
                bound,
                span,
                ctx,
                |recv, slots| {
                    render_option_ok_or_else_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("okOr", ReceiverShape::Option) => Some(render_optional_receiver_callback_call(
            object,
            bind_fixed_static_call_args(args, &["error"], &[], span, ctx)?,
            span,
            ctx,
            |recv, slots| format!("tpz_option_ok_or({recv}, {}, {})", slots[0], py_span(span)),
        )?),
        ("mapValues", ReceiverShape::Map) => {
            let bound = bind_fixed_static_call_args(args, &["f"], &[(0, 1)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(0);
            Some(render_optional_receiver_callback_call(
                object,
                bound,
                span,
                ctx,
                |recv, slots| {
                    render_map_map_values_call_with_callback(
                        recv,
                        &slots[0],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        ("update", ReceiverShape::Map) => {
            let bound =
                bind_fixed_static_call_args(args, &["k", "initial", "f"], &[(2, 1)], span, ctx)?;
            let cooperative_callback = bound.slot_is_cooperative_callback(2);
            Some(render_optional_receiver_unit_call(
                object,
                bound,
                ctx,
                |recv, slots| {
                    render_map_update_call_with_callback(
                        recv,
                        &slots[0],
                        &slots[1],
                        &slots[2],
                        span,
                        ctx.cooperative_yields,
                        cooperative_callback,
                    )
                },
            )?)
        }
        _ => None,
    };
    Ok(rendered)
}
