//! `topaz_emit_py` — Python backend parity emitter.
//!
//! The emitter mirrors the boxed Rust path over `ResolveOutput` and writes artifacts
//! through `topaz build --target python`, backed by an independent pure-Python
//! `topaz_py_rt`.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::rc::Rc;

use topaz_check::{Ctor as CheckCtor, Prim as CheckPrim, Type as CheckType};
use topaz_diag::{SourceMap, Span};
use topaz_resolve::{ResolveOutput, import_chain};
use topaz_syntax::ast::{
    ArrayElement, AssignOp, BinaryOp, Block, CallArg, CaseArmBody, CaseClause, CompBody,
    CompClause, CompKind, ConcurrentArm, EnumDecl, Expr, ExprKind, FieldInit, FunctionDecl,
    FunctionTypeParam, Ident, ImplDecl, ImportItem, ImportKind, LambdaParam, ListPatternElem,
    NewtypeDecl, Pattern, PatternKind, PipeRhs, RecordDecl, RecordPatternField, Stmt, StmtKind,
    StringLit, StringPart, Type, TypeAlias, TypeKind, UnaryOp, contains_placeholder,
};
use topaz_syntax::parse_duration_milliseconds;
use topaz_value::{
    ExternSandboxPolicy, Value, binary_value, decode_escapes, nominal_declaration_identity,
    unary_value,
};

pub const PY_RT: &str = include_str!("../py_rt/topaz_py_rt.py");
pub const SELF_PRODUCT_RT: &str = include_str!("../py_rt/topaz_self_product_rt.py");

#[path = "analysis/metadata.rs"]
mod analysis_metadata;
#[path = "analysis/mutations.rs"]
mod analysis_mutations;
#[path = "analysis/nested_forward.rs"]
mod analysis_nested_forward;
#[path = "analysis/shapes.rs"]
mod analysis_shapes;
#[path = "context.rs"]
mod context;
#[path = "emit/call/mod.rs"]
mod emit_call;
#[path = "emit/call/free.rs"]
mod emit_call_free;
#[path = "emit/call/namespace.rs"]
mod emit_call_namespace;
#[path = "emit/call/optional.rs"]
mod emit_call_optional;
#[path = "emit/call/receiver.rs"]
mod emit_call_receiver;
#[path = "emit/call/variadic.rs"]
mod emit_call_variadic;
#[path = "emit/expressions.rs"]
mod emit_expressions;
#[path = "emit/json_schema.rs"]
mod emit_json_schema;
#[path = "emit/patterns.rs"]
mod emit_patterns;
#[path = "emit/pipe/mod.rs"]
mod emit_pipe;
#[path = "emit/pipe/callbacks.rs"]
mod emit_pipe_callbacks;
#[path = "emit/pipe/spread.rs"]
mod emit_pipe_spread;
#[path = "emit/pipe/static_call.rs"]
mod emit_pipe_static_call;
#[path = "emit/statement_lowering.rs"]
mod emit_statement_lowering;
#[path = "emit/statements.rs"]
mod emit_statements;
#[path = "emit/type_specs.rs"]
mod emit_type_specs;
#[path = "model.rs"]
mod model;
#[path = "module/defaults.rs"]
mod module_defaults;
#[path = "module/definitions.rs"]
mod module_definitions;
#[path = "module/functions.rs"]
mod module_functions;
#[path = "module/imports.rs"]
mod module_imports;

pub use model::CheckedAliasSurfaces;

use analysis_metadata::*;
use analysis_mutations::*;
use analysis_nested_forward::*;
use analysis_shapes::*;
use context::*;
use emit_call::*;
use emit_call_free::*;
use emit_call_namespace::*;
use emit_call_optional::*;
use emit_call_receiver::*;
use emit_call_variadic::*;
use emit_expressions::*;
use emit_json_schema::*;
use emit_patterns::*;
use emit_pipe::*;
use emit_pipe_callbacks::*;
use emit_pipe_spread::*;
use emit_pipe_static_call::*;
use emit_statement_lowering::*;
use emit_statements::*;
use emit_type_specs::*;
use model::*;
use module_defaults::*;
use module_definitions::*;
use module_functions::*;
use module_imports::*;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Python emission failure paired with the closest available source span.
pub struct PyEmitError {
    pub kind: PyEmitErrorKind,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Closed failure classes exposed by the Python backend.
pub enum PyEmitErrorKind {
    NoEntry,
    Unsupported(&'static str),
    MalformedLiteral(&'static str),
}

impl PyEmitError {
    fn no_entry() -> Self {
        Self {
            kind: PyEmitErrorKind::NoEntry,
            span: None,
        }
    }

    fn unsupported(what: &'static str) -> Self {
        Self {
            kind: PyEmitErrorKind::Unsupported(what),
            span: None,
        }
    }

    fn malformed_literal(what: &'static str) -> Self {
        Self {
            kind: PyEmitErrorKind::MalformedLiteral(what),
            span: None,
        }
    }

    #[must_use]
    fn at(mut self, span: Span) -> Self {
        if self.span.is_none() {
            self.span = Some(span);
        }
        self
    }

    pub fn code(&self) -> &'static str {
        match self.kind {
            PyEmitErrorKind::NoEntry => "TPZ6PY0000",
            PyEmitErrorKind::Unsupported(_) => "TPZ6PY0001",
            PyEmitErrorKind::MalformedLiteral(_) => "TPZ6PY0002",
        }
    }
}

impl std::fmt::Display for PyEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            PyEmitErrorKind::NoEntry => write!(f, "{}: the unit has no entry module", self.code()),
            PyEmitErrorKind::Unsupported(what) => write!(f, "{}: unsupported {what}", self.code()),
            PyEmitErrorKind::MalformedLiteral(what) => {
                write!(f, "{}: malformed {what}", self.code())
            }
        }
    }
}

impl std::error::Error for PyEmitError {}

/// Emit a Python script that imports `topaz_py_rt` and exposes `run(stdin_text)`.
pub fn emit_module(unit: &ResolveOutput) -> Result<String, PyEmitError> {
    emit_module_with_extern_replay(unit, None)
}

/// Emits a trace-mode module with an optional deterministic extern replay log.
pub fn emit_module_with_extern_replay(
    unit: &ResolveOutput,
    extern_replay_jsonl: Option<&str>,
) -> Result<String, PyEmitError> {
    emit_module_with_extern_replay_and_policies(unit, extern_replay_jsonl, &[])
}

/// Adds explicit extern sandbox policy to trace-mode Python emission.
pub fn emit_module_with_extern_replay_and_policies(
    unit: &ResolveOutput,
    extern_replay_jsonl: Option<&str>,
    extern_sandbox_policies: &[ExternSandboxPolicy],
) -> Result<String, PyEmitError> {
    emit_module_with_checked_aliases_and_extern_replay_and_policies(
        unit,
        None,
        extern_replay_jsonl,
        extern_sandbox_policies,
    )
}

/// Emits trace-mode Python from checker-owned alias surfaces and extern inputs.
pub fn emit_module_with_checked_aliases_and_extern_replay_and_policies<'a>(
    unit: &'a ResolveOutput,
    checked_aliases: Option<&'a CheckedAliasSurfaces>,
    extern_replay_jsonl: Option<&str>,
    extern_sandbox_policies: &[ExternSandboxPolicy],
) -> Result<String, PyEmitError> {
    emit_module_with_run_mode(
        unit,
        checked_aliases,
        extern_replay_jsonl,
        extern_sandbox_policies,
        PythonRunMode::Trace,
    )
}

/// Emits application-mode host plumbing for the package's filesystem roots and extern policy.
pub fn emit_application_module_with_checked_aliases_and_extern_replay_and_policies<'a>(
    unit: &'a ResolveOutput,
    checked_aliases: Option<&'a CheckedAliasSurfaces>,
    extern_replay_jsonl: Option<&str>,
    extern_sandbox_policies: &[ExternSandboxPolicy],
    fs_read_roots: &[String],
    fs_write_roots: &[String],
) -> Result<String, PyEmitError> {
    emit_module_with_run_mode(
        unit,
        checked_aliases,
        extern_replay_jsonl,
        extern_sandbox_policies,
        PythonRunMode::Application {
            fs_read_roots,
            fs_write_roots,
        },
    )
}

fn emit_module_with_run_mode<'a>(
    unit: &'a ResolveOutput,
    checked_aliases: Option<&'a CheckedAliasSurfaces>,
    extern_replay_jsonl: Option<&str>,
    extern_sandbox_policies: &[ExternSandboxPolicy],
    run_mode: PythonRunMode<'_>,
) -> Result<String, PyEmitError> {
    let entry = unit
        .modules
        .iter()
        .find(|m| m.is_entry)
        .ok_or_else(PyEmitError::no_entry)?;
    let has_explicit_main = entry.program.items.iter().any(|stmt| {
        let StmtKind::Export(inner) = &stmt.kind else {
            return false;
        };
        let StmtKind::Function(decl) = &inner.kind else {
            return false;
        };
        text_in_map(&unit.map, decl.name.span) == "main"
    });
    let record_shapes = collect_record_shapes(unit);
    let module_default_input_catalog = collect_module_default_input_catalog(unit);
    let record_default_const_catalog =
        collect_record_default_const_catalog(unit, &module_default_input_catalog);
    let module_definition_catalog = collect_module_definition_catalog(
        unit,
        &record_default_const_catalog,
        &module_default_input_catalog,
    );
    let entry_default_inputs = module_default_input_catalog
        .get(&entry.identity)
        .expect("entry module default input facts");
    let entry_self_runtime_values = entry_default_inputs.self_runtime_values.as_ref();
    let entry_definitions = module_definition_catalog
        .get(&entry.identity)
        .expect("entry module definitions");
    let nominal_record_defs = entry_definitions.records.clone();
    let newtype_defs = entry_definitions.newtypes.clone();
    let enum_defs = entry_definitions.enums.clone();
    let entry_nominal_record_default_helpers =
        collect_nominal_record_default_helpers(&nominal_record_defs);
    let schema_modules = Rc::new(collect_json_schema_modules(
        unit,
        &module_definition_catalog,
    ));
    let all_nominal_record_defs = collect_all_nominal_record_defs(unit, &module_definition_catalog);
    let has_imported_modules = unit.modules.iter().any(|module| !module.is_entry);
    let entry_aliases = checked_aliases.and_then(|aliases| aliases.get(&entry.identity));
    let mut ctx = Ctx::new(
        &unit.map,
        nominal_record_defs,
        newtype_defs,
        enum_defs,
        entry_aliases,
    );
    ctx.module_identity = &entry.identity;
    ctx.schema_modules = Rc::clone(&schema_modules);
    register_protocols(&entry_definitions.protocol_names, &mut ctx);
    register_module_functions(
        &entry_definitions.functions,
        None,
        &entry_default_inputs.module_top_bound_names,
        &mut ctx,
    );
    enrich_module_function_mutation_metadata(&entry_definitions.functions, &mut ctx);
    enrich_module_function_return_metadata(&entry_definitions.functions, &mut ctx);
    let entry_receiver_methods =
        prepare_receiver_methods(&entry_definitions.receiver_impls, "", &mut ctx);
    let entry_protocol_methods =
        prepare_protocol_methods(&entry_definitions.protocol_impls, "", &ctx);
    let entry_receiver_method_module_values = Rc::new(receiver_method_module_value_names(
        &entry_default_inputs.module_value_source_names,
        None,
    ));

    let mut out = String::new();
    out.push_str("# Generated by topaz_emit_py. Do not edit.\n");
    out.push_str("from __future__ import annotations\n\n");
    if !record_shapes.is_empty() || !all_nominal_record_defs.is_empty() || has_imported_modules {
        out.push_str("from dataclasses import dataclass\n");
    }
    out.push_str(
        "from topaz_py_rt import Err, Host, Ok, Some, TPZ_NULL, TPZ_UNIT, TpzFault, TpzLoopBreak, TpzLoopContinue, TpzReturn, tpz_add, tpz_add_i64, tpz_array_clear, tpz_array_filter, tpz_array_filter__co, tpz_array_get, tpz_array_index_of, tpz_array_insert, tpz_array_join, tpz_array_map, tpz_array_map__co, tpz_array_pop, tpz_array_push, tpz_array_reduce, tpz_array_reduce__co, tpz_array_remove_at, tpz_array_retain, tpz_array_retain__co, tpz_array_reverse, tpz_array_slice, tpz_array_sort, tpz_array_sort_by, tpz_array_sort_by__co, tpz_array_sorted, tpz_array_sorted_by, tpz_array_sorted_by__co, tpz_bound_user_method, tpz_bytes_concat, tpz_bytes_decode_utf8, tpz_bytes_empty, tpz_bytes_encode_utf8, tpz_bytes_from_array, tpz_bytes_from_base64, tpz_bytes_from_hex, tpz_bytes_is_empty, tpz_bytes_length, tpz_bytes_slice, tpz_bytes_to_array, tpz_bytes_to_base64, tpz_bytes_to_hex, tpz_call, tpz_call_cooperative, tpz_call_order_fault, tpz_clear, tpz_coalesce, tpz_compose, tpz_concurrent_join, tpz_condition, tpz_cooperative_callable, tpz_div, tpz_div_trunc_i64, tpz_enum, tpz_enum_bare_variant_binds, tpz_enum_bare_variant_matches, tpz_enum_pattern, tpz_eq, tpz_extern_function, tpz_f64_from_bits, tpz_file_close, tpz_file_read, tpz_file_write, tpz_for_items, tpz_for_pattern, tpz_from_code_point, tpz_fs_list, tpz_fs_read_bytes, tpz_fs_read_text, tpz_fs_write_bytes, tpz_fs_write_text, tpz_ge, tpz_ge_i64, tpz_get, tpz_gt, tpz_gt_i64, tpz_host_callable, tpz_immutable_assignment, tpz_impossible_match, tpz_in, tpz_index, tpz_index_slot, tpz_index_slot_get, tpz_index_slot_is_empty, tpz_index_slot_set, tpz_is_empty, tpz_is_enum, tpz_is_newtype, tpz_is_nominal_record, tpz_json_as_bool, tpz_json_as_int, tpz_json_as_string, tpz_json_at, tpz_json_get, tpz_json_is_null, tpz_json_kind, tpz_json_length, tpz_json_number_text, tpz_json_parse, tpz_json_stringify, tpz_le, tpz_le_i64, tpz_length, tpz_let_pattern, tpz_lt, tpz_lt_i64, tpz_make_template, tpz_map_clear, tpz_map_contains_key, tpz_map_filter, tpz_map_filter__co, tpz_map_get, tpz_map_get_or, tpz_map_insert, tpz_map_map_values, tpz_map_map_values__co, tpz_map_new, tpz_map_of, tpz_map_of_entries, tpz_map_remove, tpz_map_update, tpz_map_update__co, tpz_member, tpz_mul, tpz_mul_i64, tpz_ne, tpz_neg, tpz_newtype, tpz_newtype_unwrap, tpz_nominal_record, tpz_nominal_record__co, tpz_nonvariadic_spread_call, tpz_nonvariadic_static_spread_call, tpz_option_flat_map, tpz_option_flat_map__co, tpz_option_map, tpz_option_map__co, tpz_option_ok_or, tpz_option_ok_or_else, tpz_option_ok_or_else__co, tpz_optional_member, tpz_pow, tpz_pow_i64, tpz_range, tpz_record_field, tpz_record_update, tpz_rem_trunc_i64, tpz_remove, tpz_render, tpz_result_flat_map, tpz_result_flat_map__co, tpz_result_map, tpz_result_map__co, tpz_return, tpz_run_defer, tpz_set_add, tpz_set_contains, tpz_set_difference, tpz_set_intersection, tpz_set_is_empty, tpz_set_of, tpz_set_remove, tpz_set_to_array, tpz_set_union, tpz_spread_values, tpz_string_byte_length, tpz_string_code_point_at, tpz_string_contains, tpz_string_ends_with, tpz_string_index_of, tpz_string_last_index_of, tpz_string_replace, tpz_string_slice, tpz_string_split, tpz_string_starts_with, tpz_string_trim, tpz_string_trim_end, tpz_string_trim_start, tpz_sub, tpz_sub_i64, tpz_to_array, tpz_to_int, tpz_try, tpz_type_matches, tpz_wrap_optional, tpz_wrap_optional_unit\n\n",
    );
    out.push_str("from topaz_py_rt import tpz_concurrent_join_timeout\n\n");
    out.push_str(
        "from topaz_py_rt import TpzByteBuffer, tpz_byte_buffer_allocate, tpz_byte_buffer_copy, tpz_byte_buffer_fill, tpz_byte_buffer_from_bytes, tpz_byte_buffer_get, tpz_byte_buffer_length, tpz_byte_buffer_set, tpz_byte_buffer_to_bytes\n\n",
    );
    out.push_str("from topaz_py_rt import tpz_using_file\n\n");
    out.push_str("from topaz_py_rt import tpz_method_registry\n\n");
    out.push_str(
        "from topaz_py_rt import tpz_user_method_call, tpz_user_method_call_cooperative\n\n",
    );
    out.push_str("from topaz_py_rt import tpz_protocol_call, tpz_protocol_call_cooperative\n\n");
    out.push_str("from topaz_py_rt import tpz_json_decode, tpz_json_parse_as\n\n");
    if matches!(run_mode, PythonRunMode::Application { .. }) {
        out.push_str("from topaz_py_rt import DeploymentHost\n\n");
    }
    out.push_str(
        "from topaz_py_rt import tpz_cli_has_flag, tpz_cli_option, tpz_cli_options, tpz_cli_positionals, tpz_csv_parse_with_header, tpz_hash_crc32, tpz_hash_hmac_sha256, tpz_hash_sha256, tpz_hash_sha512, tpz_json_as_array, tpz_json_keys, tpz_json_values, tpz_math_abs, tpz_math_ceil, tpz_math_cos, tpz_math_floor, tpz_math_is_finite, tpz_math_is_nan, tpz_math_max, tpz_math_min, tpz_math_parse_float, tpz_math_round, tpz_math_sin, tpz_math_sqrt, tpz_math_tan, tpz_regex_compile, tpz_regex_is_match, tpz_regex_replace_all, tpz_regex_split, tpz_split, tpz_toml_parse, tpz_toml_to_json, tpz_url_parse, tpz_url_path, tpz_url_to_string\n\n",
    );
    writeln!(
        out,
        "_TPZ_EXTERN_REPLAY_JSONL = {}\n",
        py_string(extern_replay_jsonl.unwrap_or(""))
    )
    .expect("write to string");
    writeln!(
        out,
        "_TPZ_EXTERN_SANDBOX_POLICIES_JSON = {}\n",
        py_string(&extern_sandbox_policies_json(extern_sandbox_policies))
    )
    .expect("write to string");
    out.push_str("__tpz_missing = object()\n\n");
    out.push_str("__tpz_methods = tpz_method_registry()\n\n");
    out.push_str("def __tpz_self_default(value, name, span):\n");
    out.push_str("    if value is __tpz_missing:\n");
    out.push_str("        raise TpzFault(\"TPZ5002\", f\"`{name}` is not bound\", span)\n");
    out.push_str("    return value\n\n");
    out.push_str("def __tpz_module_value(py_name, name, span):\n");
    out.push_str("    value = globals().get(py_name, __tpz_missing)\n");
    out.push_str("    if value is __tpz_missing:\n");
    out.push_str("        raise TpzFault(\"TPZ5002\", f\"`{name}` is not bound\", span)\n");
    out.push_str("    return value\n\n");
    out.push_str("def __tpz_forward_function(cell, name, span):\n");
    out.push_str("    value = cell[0]\n");
    out.push_str("    if value is __tpz_missing:\n");
    out.push_str("        raise TpzFault(\"TPZ5002\", f\"`{name}` is not bound\", span)\n");
    out.push_str("    return value\n\n");
    emit_record_classes(&record_shapes, &mut out);
    emit_nominal_record_classes(&all_nominal_record_defs, &mut out);
    let (module_exports, module_inits) = emit_imported_module_values(
        unit,
        checked_aliases,
        &module_default_input_catalog,
        &record_default_const_catalog,
        &module_definition_catalog,
        Rc::clone(&schema_modules),
        &mut out,
    )?;

    let mut run_body = String::new();
    let mut run_trace_value = false;
    run_body.push_str("        __tpz_methods.clear()\n");
    write_receiver_method_module_value_seeds(
        &mut run_body,
        8,
        &entry_receiver_method_module_values,
    );
    for init in &module_inits {
        writeln!(run_body, "        {init}(host)").expect("write to string");
    }
    emit_receiver_method_registrations(&entry_receiver_methods, 8, &mut run_body);
    emit_protocol_method_registrations(&entry_protocol_methods, 8, &mut run_body);
    write_self_runtime_default_py_seeds(&mut run_body, 8, entry_self_runtime_values, None);
    let entry_self_runtime_sources =
        self_runtime_default_py_source_names(entry_self_runtime_values, None);
    for (idx, stmt) in entry.program.items.iter().enumerate() {
        let is_last = idx + 1 == entry.program.items.len();
        let item = exported_inner(stmt);
        if stmt_has_bare_return(item) {
            return Err(PyEmitError::unsupported("return outside a function").at(item.span));
        }
        match &item.kind {
            StmtKind::Import(import) => {
                emit_import_binding(import, &module_exports, &mut ctx, &mut run_body, 8)?;
                enrich_module_function_mutation_metadata(&entry_definitions.functions, &mut ctx);
            }
            StmtKind::Function(decl) => {
                emit_function(decl, &mut ctx, &mut out)?;
                out.push('\n');
            }
            StmtKind::Record(_)
            | StmtKind::TypeAlias(_)
            | StmtKind::Impl(_)
            | StmtKind::Protocol(_) => {}
            StmtKind::Let {
                mutable,
                pattern,
                ty,
                value,
            } => {
                if !matches!(
                    pattern.kind,
                    PatternKind::Binding(_) | PatternKind::Typed { .. }
                ) {
                    emit_global_destructuring_let(
                        pattern,
                        *mutable,
                        value,
                        item.span,
                        StatementEmission::new(&mut ctx, 8, &mut run_body),
                        mangle,
                    )?;
                    continue;
                }
                let source_name = ctx.binding_name(pattern)?.to_string();
                emit_global_value_binding(
                    ValueBindingInput {
                        source_name: &source_name,
                        mutable: *mutable,
                        value,
                        annotation: ty.as_ref().or_else(|| pattern_type(pattern)),
                        runtime_guard: (!*mutable).then(|| pattern_type(pattern)).flatten(),
                        span: item.span,
                    },
                    mangle(&source_name),
                    entry_self_runtime_sources
                        .get(source_name.as_str())
                        .map(String::as_str),
                    &mut ctx,
                    8,
                    &mut run_body,
                )?;
            }
            StmtKind::Const { name, ty, value } => {
                let source_name = ctx.text(name.span).to_string();
                emit_global_value_binding(
                    ValueBindingInput {
                        source_name: &source_name,
                        mutable: false,
                        value,
                        annotation: ty.as_ref(),
                        runtime_guard: None,
                        span: item.span,
                    },
                    mangle(&source_name),
                    None,
                    &mut ctx,
                    8,
                    &mut run_body,
                )?;
            }
            StmtKind::Expr(expr) if is_last && should_trace_final_expr(expr, &ctx) => {
                if !emit_expr_to_target_if_needed(expr, "__tpz_value", &mut ctx, 8, &mut run_body)?
                {
                    let value_py = emit_expr(expr, &ctx)?;
                    writeln!(run_body, "        __tpz_value = {value_py}")
                        .expect("write to string");
                }
                run_trace_value = true;
            }
            _ => emit_stmt(item, &mut ctx, 8, &mut run_body)?,
        }
    }

    if has_explicit_main {
        let info = ctx
            .function_info("main")
            .expect("registered exported main function metadata");
        let call = if info.needs_host {
            format!(
                "{}(host, [] if args is None else args, stdin_text)",
                info.py_name
            )
        } else {
            format!("{}([] if args is None else args, stdin_text)", info.py_name)
        };
        writeln!(run_body, "        __tpz_main_result = {call}").expect("write to string");
    }

    emit_receiver_method_functions(
        &entry_receiver_methods,
        &entry_receiver_method_module_values,
        &mut ctx,
        &mut out,
    )?;
    emit_protocol_method_functions(
        &entry_protocol_methods,
        &entry_receiver_method_module_values,
        &mut ctx,
        &mut out,
    )?;

    emit_nominal_record_default_helpers(&entry_nominal_record_default_helpers, &mut ctx, &mut out)?;

    match run_mode {
        PythonRunMode::Trace => {
            out.push_str("\ndef run(stdin_text: str, files: dict[str, str] | None = None, extern_replay_jsonl: str | None = None, extern_sandbox_policies_json: str | None = None, args: list[str] | None = None) -> str:\n");
            out.push_str("    host = Host(stdin_text, files, _TPZ_EXTERN_REPLAY_JSONL if extern_replay_jsonl is None else extern_replay_jsonl, _TPZ_EXTERN_SANDBOX_POLICIES_JSON if extern_sandbox_policies_json is None else extern_sandbox_policies_json)\n");
        }
        PythonRunMode::Application {
            fs_read_roots,
            fs_write_roots,
        } => {
            writeln!(
                out,
                "\n_TPZ_FS_READ_ROOTS = {}",
                py_string_list(fs_read_roots)
            )
            .expect("write to string");
            writeln!(
                out,
                "_TPZ_FS_WRITE_ROOTS = {}",
                py_string_list(fs_write_roots)
            )
            .expect("write to string");
            out.push_str("\ndef run(stdin_text: str, args: list[str] | None = None) -> int:\n");
            out.push_str("    host = DeploymentHost(stdin_text, \".\", _TPZ_FS_READ_ROOTS, _TPZ_FS_WRITE_ROOTS, _TPZ_EXTERN_REPLAY_JSONL, _TPZ_EXTERN_SANDBOX_POLICIES_JSON)\n");
        }
    }
    out.push_str("    __tpz_defers = []\n");
    emit_defer_helpers(&mut out, 4);
    out.push_str("    try:\n");
    if run_body.is_empty() {
        out.push_str("        pass\n");
    } else {
        out.push_str(&run_body);
    }
    out.push_str("        __tpz_run_defers()\n");
    match run_mode {
        PythonRunMode::Trace if run_trace_value => {
            out.push_str("        return host.trace_ok(__tpz_value)\n");
        }
        PythonRunMode::Trace => out.push_str("        return host.trace_ok()\n"),
        PythonRunMode::Application { .. } if has_explicit_main => {
            out.push_str("        return host.application_exit(__tpz_main_result)\n");
        }
        PythonRunMode::Application { .. } => out.push_str("        return 0\n"),
    }
    out.push_str("    except TpzFault as fault:\n");
    match run_mode {
        PythonRunMode::Trace => out.push_str("        return host.trace_fault(fault)\n\n"),
        PythonRunMode::Application { .. } => {
            out.push_str("        return host.application_fault(fault)\n\n")
        }
    }
    out.push_str("if __name__ == \"__main__\":\n");
    out.push_str("    import sys\n");
    match run_mode {
        PythonRunMode::Trace => {
            out.push_str("    sys.stdout.write(run(sys.stdin.read(), args=sys.argv[1:]))\n");
            out.push_str("    sys.stdout.write(\"\\n\")\n");
        }
        PythonRunMode::Application { .. } => {
            out.push_str("    raise SystemExit(run(sys.stdin.read(), args=sys.argv[1:]))\n")
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
