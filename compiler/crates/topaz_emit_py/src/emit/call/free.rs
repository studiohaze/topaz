use crate::*;

pub(super) fn emit_call(
    callee: &Expr,
    args: &[CallArg],
    span: Span,
    ctx: &Ctx<'_>,
) -> Result<String, PyEmitError> {
    if let ExprKind::Ident = &callee.kind {
        let name = ctx.text(callee.span);
        if let Some(info) = ctx.function_info(name)
            && !ctx.binding_is_bound(name)
        {
            return emit_known_function_call(info, args, span, ctx);
        }
        if ctx.binding_is_bound(name) {
            if let Some(info) = ctx.binding_callable_info_at(name, callee.span) {
                return emit_known_function_call(&info, args, span, ctx);
            }
            if ctx.binding_is_composed(name) {
                return emit_composed_binding_call(name, args, span, ctx);
            }
            return Err(PyEmitError::unsupported("call target").at(callee.span));
        }
        if let Some(newtype) = ctx.newtypes.get(name) {
            return emit_newtype_construct(newtype, args, span, ctx);
        }
        if let Some(call) = emit_free_builtin_call(name, args, span, ctx)? {
            return Ok(call);
        }
        positional_args(args)?;
        return Err(PyEmitError::unsupported("call target").at(callee.span));
    }
    if let ExprKind::Paren(inner) = &callee.kind {
        return emit_call(inner, args, span, ctx);
    }
    if let ExprKind::OptionalAccess { object, field } = &callee.kind {
        let method = ctx.text(field.span);
        if let Some(params) = ctx
            .option_record_field_projection(object, field)
            .callable_params
        {
            return emit_optional_static_callable_value_call(
                object, field, args, &params, span, ctx,
            );
        }
        if let Some(call) =
            emit_optional_receiver_callback_builtin_call(object, method, args, span, ctx)?
        {
            return Ok(call);
        }
        if let Some(call) =
            emit_optional_receiver_readonly_builtin_call(object, method, args, span, ctx)?
        {
            return Ok(call);
        }
        return Err(PyEmitError::unsupported("call target").at(callee.span));
    }
    if let ExprKind::Member { object, field } = &callee.kind {
        let method = ctx.text(field.span);
        if let ExprKind::Ident = &object.kind {
            let namespace = ctx.text(object.span);
            if !ctx.binding_is_bound(namespace)
                && let Some(enum_def) = ctx.enums.get(namespace)
                && let Some(variant) = enum_def.variants.get(method)
            {
                return emit_enum_construct(enum_def, method, variant, args, span, ctx);
            }
            if !ctx.binding_is_bound(namespace)
                && !ctx.namespaces.contains_key(namespace)
                && ctx.protocols.contains(namespace)
            {
                let positional = positional_args(args)?;
                let emitted_args = positional
                    .iter()
                    .map(|arg| emit_expr(arg, ctx))
                    .collect::<Result<Vec<_>, _>>()?;
                let module = ctx.method_module_identity.unwrap_or("__entry__");
                let helper = if ctx.cooperative_yields {
                    "tpz_protocol_call_cooperative"
                } else {
                    "tpz_protocol_call"
                };
                let call = format!(
                    "{helper}(__tpz_methods, {}, {}, {}, [{}], {})",
                    py_string(module),
                    py_string(namespace),
                    py_string(method),
                    emitted_args.join(", "),
                    py_span(span)
                );
                return Ok(if ctx.cooperative_yields {
                    format!("(yield from {call})")
                } else {
                    call
                });
            }
            if let Some(ModuleRuntimeExport::Function { info }) =
                ctx.namespace_export(namespace, method)
            {
                return emit_known_function_call(info, args, span, ctx);
            }
            if let Some(ModuleRuntimeExport::Value { metadata, .. }) =
                ctx.namespace_export(namespace, method)
                && let Some(params) = metadata.callable_params.as_ref()
            {
                return emit_static_callable_value_call(callee, args, params, span, ctx);
            }
            if let Some(call) = emit_namespace_builtin_call(namespace, method, args, span, ctx)? {
                return Ok(call);
            }
            if let Some(call) =
                emit_receiver_readonly_builtin_call(object, method, args, span, ctx)?
            {
                return Ok(call);
            }
            if let Some(call) =
                emit_receiver_callback_builtin_call(object, method, args, span, ctx)?
            {
                return Ok(call);
            }
            if let Some(call) =
                emit_receiver_mutating_spread_builtin_call(object, method, args, span, ctx)?
            {
                return Ok(call);
            }
            if let Some(params) = ctx
                .record_member_field_projection(object, field)
                .callable_params
            {
                return emit_static_callable_value_call(callee, args, &params, span, ctx);
            }
            if let Some(info) = ctx.receiver_method_info(method) {
                let receiver_py = emit_expr(object, ctx)?;
                let callee_py = format!(
                    "tpz_bound_user_method(__tpz_methods, {receiver_py}, {}, {}, {})",
                    py_string(method),
                    py_string(&mangle(method)),
                    py_span(callee.span),
                );
                return emit_user_receiver_method_call(
                    callee_py,
                    args,
                    info.params.get(1..).unwrap_or(&[]),
                    span,
                    ctx,
                );
            }
            if ctx.receiver_method_known(method) {
                let receiver_py = emit_expr(object, ctx)?;
                let callee_py = format!(
                    "tpz_bound_user_method(__tpz_methods, {receiver_py}, {}, {}, {})",
                    py_string(method),
                    py_string(&mangle(method)),
                    py_span(callee.span),
                );
                return emit_dynamic_user_receiver_method_call(callee_py, args, span, ctx);
            }
            let positional = positional_args(args)?;
            let emitted_args = positional
                .iter()
                .map(|arg| emit_expr(arg, ctx))
                .collect::<Result<Vec<_>, _>>()?;
            if namespace == "JSON" && method == "parse" && emitted_args.len() == 1 {
                return Ok(format!(
                    "tpz_json_parse({}, {})",
                    emitted_args[0],
                    py_span(span)
                ));
            }
            if namespace == "JSON" && method == "stringify" && emitted_args.len() == 1 {
                return Ok(format!("tpz_json_stringify({})", emitted_args[0]));
            }
            if namespace == "Map" && method == "new" && emitted_args.is_empty() {
                return Ok("tpz_map_new()".to_string());
            }
            if namespace == "Set" && method == "of" {
                return Ok(format!(
                    "tpz_set_of([{}], {})",
                    emitted_args.join(", "),
                    py_span(span)
                ));
            }
            if namespace == "Bytes" && method == "empty" && emitted_args.is_empty() {
                return Ok("tpz_bytes_empty()".to_string());
            }
            if namespace == "Bytes" && method == "encodeUtf8" && emitted_args.len() == 1 {
                return Ok(format!(
                    "tpz_bytes_encode_utf8({}, {})",
                    emitted_args[0],
                    py_span(span)
                ));
            }
            if namespace == "Bytes" && method == "fromArray" && emitted_args.len() == 1 {
                return Ok(format!(
                    "tpz_bytes_from_array({}, {})",
                    emitted_args[0],
                    py_span(span)
                ));
            }
            if namespace == "Bytes" && method == "fromHex" && emitted_args.len() == 1 {
                return Ok(format!(
                    "tpz_bytes_from_hex({}, {})",
                    emitted_args[0],
                    py_span(span)
                ));
            }
            if namespace == "Bytes" && method == "fromBase64" && emitted_args.len() == 1 {
                return Ok(format!(
                    "tpz_bytes_from_base64({}, {})",
                    emitted_args[0],
                    py_span(span)
                ));
            }
            if namespace == "Bytes" && method == "concat" && emitted_args.len() == 2 {
                return Ok(format!(
                    "tpz_bytes_concat({}, {}, {})",
                    emitted_args[0],
                    emitted_args[1],
                    py_span(span)
                ));
            }
            if namespace == "Encoding" && method == "utf8Encode" && emitted_args.len() == 1 {
                return Ok(format!(
                    "tpz_bytes_encode_utf8({}, {})",
                    emitted_args[0],
                    py_span(span)
                ));
            }
            if namespace == "Encoding" && method == "utf8Decode" && emitted_args.len() == 1 {
                return Ok(format!(
                    "tpz_bytes_decode_utf8({}, {})",
                    emitted_args[0],
                    py_span(span)
                ));
            }
            if namespace == "Encoding" && method == "hexEncode" && emitted_args.len() == 1 {
                return Ok(format!(
                    "tpz_bytes_to_hex({}, {})",
                    emitted_args[0],
                    py_span(span)
                ));
            }
            if namespace == "Encoding" && method == "hexDecode" && emitted_args.len() == 1 {
                return Ok(format!(
                    "tpz_bytes_from_hex({}, {})",
                    emitted_args[0],
                    py_span(span)
                ));
            }
            if namespace == "Encoding" && method == "base64Encode" && emitted_args.len() == 1 {
                return Ok(format!(
                    "tpz_bytes_to_base64({}, {})",
                    emitted_args[0],
                    py_span(span)
                ));
            }
            if namespace == "Encoding" && method == "base64Decode" && emitted_args.len() == 1 {
                return Ok(format!(
                    "tpz_bytes_from_base64({}, {})",
                    emitted_args[0],
                    py_span(span)
                ));
            }
        }
        if let Some(info) = ctx.receiver_method_info(method) {
            let receiver_py = emit_expr(object, ctx)?;
            let callee_py = format!(
                "tpz_bound_user_method(__tpz_methods, {receiver_py}, {}, {}, {})",
                py_string(method),
                py_string(&mangle(method)),
                py_span(callee.span),
            );
            return emit_user_receiver_method_call(
                callee_py,
                args,
                info.params.get(1..).unwrap_or(&[]),
                span,
                ctx,
            );
        }
        if ctx.receiver_method_known(method) {
            let receiver_py = emit_expr(object, ctx)?;
            let callee_py = format!(
                "tpz_bound_user_method(__tpz_methods, {receiver_py}, {}, {}, {})",
                py_string(method),
                py_string(&mangle(method)),
                py_span(callee.span),
            );
            return emit_dynamic_user_receiver_method_call(callee_py, args, span, ctx);
        }
        if let Some(call) = emit_receiver_readonly_builtin_call(object, method, args, span, ctx)? {
            return Ok(call);
        }
        if let Some(call) = emit_receiver_callback_builtin_call(object, method, args, span, ctx)? {
            return Ok(call);
        }
        if let Some(call) =
            emit_receiver_mutating_spread_builtin_call(object, method, args, span, ctx)?
        {
            return Ok(call);
        }
        if let Some(params) = ctx
            .record_member_field_projection(object, field)
            .callable_params
        {
            return emit_static_callable_value_call(callee, args, &params, span, ctx);
        }
        let positional = positional_args(args)?;
        let emitted_args = positional
            .iter()
            .map(|arg| emit_expr(arg, ctx))
            .collect::<Result<Vec<_>, _>>()?;
        if method == "split" && emitted_args.len() == 1 {
            return Ok(format!(
                "tpz_string_split({}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(span)
            ));
        }
        if method == "codePointAt" && emitted_args.len() == 1 {
            return Ok(format!(
                "tpz_string_code_point_at({}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(span)
            ));
        }
        if method == "byteLength" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_string_byte_length({}, {})",
                emit_expr(object, ctx)?,
                py_span(span)
            ));
        }
        if method == "scalars" && emitted_args.is_empty() {
            // CPython str iteration yields Unicode scalar strings, matching this witness subset.
            return Ok(format!("list({})", emit_expr(object, ctx)?));
        }
        if method == "map" && emitted_args.len() == 1 && receiver_is_array_value(object, ctx) {
            let callback = emit_callback_expr(positional[0], 1, ctx)?;
            return Ok(render_array_map_call_with_callback(
                &emit_expr(object, ctx)?,
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "map" && emitted_args.len() == 1 && receiver_is_option_value(object, ctx) {
            let callback = emit_callback_expr(positional[0], 1, ctx)?;
            return Ok(render_option_map_call_with_callback(
                &emit_expr(object, ctx)?,
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "map" && emitted_args.len() == 1 && receiver_is_result_value(object, ctx) {
            let callback = emit_callback_expr(positional[0], 1, ctx)?;
            return Ok(render_result_map_call_with_callback(
                &emit_expr(object, ctx)?,
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "flatMap" && emitted_args.len() == 1 && receiver_is_option_value(object, ctx) {
            let callback = emit_callback_expr(positional[0], 1, ctx)?;
            return Ok(render_option_flat_map_call_with_callback(
                &emit_expr(object, ctx)?,
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "okOrElse" && emitted_args.len() == 1 && receiver_is_option_value(object, ctx)
        {
            let callback = emit_callback_expr(positional[0], 0, ctx)?;
            return Ok(render_option_ok_or_else_call_with_callback(
                &emit_expr(object, ctx)?,
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "okOr" && emitted_args.len() == 1 && receiver_is_option_value(object, ctx) {
            return Ok(format!(
                "tpz_option_ok_or({}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(span)
            ));
        }
        if method == "flatMap" && emitted_args.len() == 1 && receiver_is_result_value(object, ctx) {
            let callback = emit_callback_expr(positional[0], 1, ctx)?;
            return Ok(render_result_flat_map_call_with_callback(
                &emit_expr(object, ctx)?,
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "filter" && emitted_args.len() == 1 && receiver_is_array_value(object, ctx) {
            let callback = emit_callback_expr(positional[0], 1, ctx)?;
            return Ok(render_array_filter_call_with_callback(
                &emit_expr(object, ctx)?,
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "filter" && emitted_args.len() == 1 && receiver_is_map_value(object, ctx) {
            let callback = emit_callback_expr(positional[0], 2, ctx)?;
            return Ok(render_map_filter_call_with_callback(
                &emit_expr(object, ctx)?,
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "reduce" && emitted_args.len() == 2 && receiver_is_array_value(object, ctx) {
            let callback = emit_callback_expr(positional[1], 2, ctx)?;
            return Ok(render_array_reduce_call_with_callback(
                &emit_expr(object, ctx)?,
                &emitted_args[0],
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "sorted" && emitted_args.is_empty() && receiver_is_array_value(object, ctx) {
            return Ok(format!(
                "tpz_array_sorted({}, {})",
                emit_expr(object, ctx)?,
                py_span(span)
            ));
        }
        if method == "sort" && emitted_args.is_empty() && receiver_is_array_value(object, ctx) {
            return Ok(format!(
                "tpz_array_sort({}, {})",
                emit_expr(object, ctx)?,
                py_span(span)
            ));
        }
        if method == "sortedBy" && emitted_args.len() == 1 && receiver_is_array_value(object, ctx) {
            let callback = emit_callback_expr(positional[0], 1, ctx)?;
            return Ok(render_array_sorted_by_call_with_callback(
                &emit_expr(object, ctx)?,
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "sortBy" && emitted_args.len() == 1 && receiver_is_array_value(object, ctx) {
            let callback = emit_callback_expr(positional[0], 1, ctx)?;
            return Ok(render_array_sort_by_call_with_callback(
                &emit_expr(object, ctx)?,
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "retain" && emitted_args.len() == 1 && receiver_is_array_value(object, ctx) {
            let callback = emit_callback_expr(positional[0], 1, ctx)?;
            return Ok(render_array_retain_call_with_callback(
                &emit_expr(object, ctx)?,
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "mapValues" && emitted_args.len() == 1 && receiver_is_map_value(object, ctx) {
            let callback = emit_callback_expr(positional[0], 1, ctx)?;
            return Ok(render_map_map_values_call_with_callback(
                &emit_expr(object, ctx)?,
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "update" && emitted_args.len() == 3 && receiver_is_map_value(object, ctx) {
            let callback = emit_callback_expr(positional[2], 1, ctx)?;
            return Ok(render_map_update_call_with_callback(
                &emit_expr(object, ctx)?,
                &emitted_args[0],
                &emitted_args[1],
                &callback.py,
                span,
                ctx.cooperative_yields,
                callback.cooperative_callback,
            ));
        }
        if method == "get" && emitted_args.len() == 1 {
            return Ok(format!(
                "tpz_get({}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(span)
            ));
        }
        if method == "getOr" && emitted_args.len() == 2 {
            return Ok(format!(
                "tpz_map_get_or({}, {}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                emitted_args[1],
                py_span(span)
            ));
        }
        if method == "containsKey" && emitted_args.len() == 1 {
            return Ok(format!(
                "tpz_map_contains_key({}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(span)
            ));
        }
        if method == "insert" && emitted_args.len() == 2 && receiver_is_map_value(object, ctx) {
            return Ok(format!(
                "tpz_map_insert({}, {}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                emitted_args[1],
                py_span(span)
            ));
        }
        if method == "remove" && emitted_args.len() == 1 {
            return Ok(format!(
                "tpz_remove({}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(span)
            ));
        }
        if method == "clear" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_clear({}, {})",
                emit_expr(object, ctx)?,
                py_span(span)
            ));
        }
        if method == "add" && emitted_args.len() == 1 {
            return Ok(format!(
                "tpz_set_add({}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(span)
            ));
        }
        if method == "contains" && emitted_args.len() == 1 {
            return Ok(format!(
                "tpz_set_contains({}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(span)
            ));
        }
        if method == "union" && emitted_args.len() == 1 {
            return Ok(format!(
                "tpz_set_union({}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(span)
            ));
        }
        if method == "intersection" && emitted_args.len() == 1 {
            return Ok(format!(
                "tpz_set_intersection({}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(span)
            ));
        }
        if method == "difference" && emitted_args.len() == 1 {
            return Ok(format!(
                "tpz_set_difference({}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(span)
            ));
        }
        if method == "decodeUtf8" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_bytes_decode_utf8({}, {})",
                emit_expr(object, ctx)?,
                py_span(span)
            ));
        }
        if method == "toHex" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_bytes_to_hex({}, {})",
                emit_expr(object, ctx)?,
                py_span(span)
            ));
        }
        if method == "toBase64" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_bytes_to_base64({}, {})",
                emit_expr(object, ctx)?,
                py_span(span)
            ));
        }
        if method == "isEmpty" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_is_empty({}, {})",
                emit_expr(object, ctx)?,
                py_span(span)
            ));
        }
        if method == "slice" && emitted_args.len() == 2 && receiver_is_bytes_value(object, ctx) {
            return Ok(format!(
                "tpz_bytes_slice({}, {}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                emitted_args[1],
                py_span(span)
            ));
        }
        if method == "toArray" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_to_array({}, {})",
                emit_expr(object, ctx)?,
                py_span(span)
            ));
        }
        if method == "push" && emitted_args.len() == 1 && receiver_is_array_value(object, ctx) {
            return Ok(format!(
                "tpz_array_push({}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(span)
            ));
        }
        if method == "read" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_file_read({}, {})",
                emit_expr(object, ctx)?,
                py_span(field.span)
            ));
        }
        if method == "write" && emitted_args.len() == 1 {
            return Ok(format!(
                "tpz_file_write({}, {}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(field.span),
                py_span(span)
            ));
        }
        if method == "close" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_file_close({}, {})",
                emit_expr(object, ctx)?,
                py_span(field.span)
            ));
        }
        if method == "kind" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_json_kind({}, {})",
                emit_expr(object, ctx)?,
                py_span(field.span)
            ));
        }
        if method == "isNull" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_json_is_null({}, {})",
                emit_expr(object, ctx)?,
                py_span(field.span)
            ));
        }
        if method == "asString" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_json_as_string({}, {})",
                emit_expr(object, ctx)?,
                py_span(field.span)
            ));
        }
        if method == "asBool" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_json_as_bool({}, {})",
                emit_expr(object, ctx)?,
                py_span(field.span)
            ));
        }
        if method == "asInt" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_json_as_int({}, {})",
                emit_expr(object, ctx)?,
                py_span(field.span)
            ));
        }
        if method == "numberText" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_json_number_text({}, {})",
                emit_expr(object, ctx)?,
                py_span(field.span)
            ));
        }
        if method == "get" && emitted_args.len() == 1 {
            return Ok(format!(
                "tpz_json_get({}, {}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(field.span),
                py_span(span)
            ));
        }
        if method == "at" && emitted_args.len() == 1 {
            return Ok(format!(
                "tpz_json_at({}, {}, {}, {})",
                emit_expr(object, ctx)?,
                emitted_args[0],
                py_span(field.span),
                py_span(span)
            ));
        }
        if method == "length" && emitted_args.is_empty() {
            return Ok(format!(
                "tpz_length({}, {})",
                emit_expr(object, ctx)?,
                py_span(field.span)
            ));
        }
        return Err(PyEmitError::unsupported("member call").at(field.span));
    }
    if let ExprKind::Index { object, index } = &callee.kind {
        if let Some(params) = ctx.array_element_callable_params_for_index(object, index) {
            return emit_static_callable_value_call(callee, args, &params, span, ctx);
        }
        return emit_positional_callable_value_call(callee, args, span, ctx);
    }
    if let ExprKind::Compose { .. } = &callee.kind {
        return emit_positional_callable_value_call(callee, args, span, ctx);
    }
    if lambda_callee(callee) {
        let positional = positional_args(args)?;
        let emitted_args = positional
            .iter()
            .map(|arg| emit_expr(arg, ctx))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(format!(
            "{}({})",
            emit_expr(callee, ctx)?,
            emitted_args.join(", ")
        ));
    }
    Err(PyEmitError::unsupported("call target").at(callee.span))
}
