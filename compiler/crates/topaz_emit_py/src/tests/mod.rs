//! Python-emitter regression families and their shared execution harness.
//! Source construction, CPython invocation, and trace comparison stay here;
//! production emitters expose no test-only entry points.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use topaz_diag::{FileId, Span};
use topaz_resolve::{InMemoryProvider, PhysicalProvider, ResolveOutput, resolve_with_version};
use topaz_syntax::LangVersion;
use topaz_syntax::ast::{Expr, ExprKind, FunctionDecl, StmtKind, StringLit};
use topaz_value::{
    CANONICAL_ARITHMETIC_NAN_BITS, FLOAT_RENDER_GOLDENS, RtError, Value, float_arith, int_add,
    int_div, int_mul, int_pow, int_rem, int_sub, record_update_base, record_update_merge,
    render_float,
};

use super::{
    Ctx, DirectTailMetadata, PY_RT, PyEmitError, PyEmitErrorKind, ReceiverShape, RecordWrapper,
    collect_module_binding_facts, collect_module_default_input_facts, collect_record_shapes,
    direct_tail_expr_return_shape, direct_tail_metadata, emit_module,
    emit_module_with_checked_aliases_and_extern_replay_and_policies, exported_inner, function_info,
    mangle, module_top_bound_names_for_direct_tail, py_string,
    reject_yield_from_inside_optional_receiver_unit_lambda, statement_lowered_yield_from_call_expr,
    text_in_map,
};

const SP: Span = Span {
    file: FileId(7),
    lo: 11,
    hi: 13,
};
static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

mod callbacks;
mod calls_free_dynamic;
mod calls_free_map_patterns;
mod calls_free_spread_faults;
mod calls_free_wrapped_records;
mod calls_namespace;
mod calls_optional;
mod calls_receiver;
mod collections;
mod concurrent_callback_routing;
mod concurrent_control;
mod concurrent_map_callbacks;
mod concurrent_no_timeout_callbacks;
mod concurrent_storage_callbacks;
mod concurrent_timeout;
mod functions;
mod json;
mod module_defaults;
mod nested;
mod patterns;
mod pipes_imports;
mod pipes_namespace_aliases;
mod pipes_namespace_calls;
mod pipes_unproven_namespace_alias;
mod regressions;
mod statements_callable_mutations;
mod statements_lowering;
mod statements_mutations;
mod statements_records;
mod support;
mod types_aliases;
mod types_callable_arrays;
mod types_dynamic_pipe_calls;
mod types_mutable_calls;
mod types_record_field_calls;
mod types_unproven_record_calls;

use support::*;
