//! Audited target-product runtime for Topaz compiler IR.
//!
//! This crate deliberately has no dependency on the lexer, parser, resolver,
//! checker, lowerer, emitter, interpreter, or self-front-end crates. It only
//! executes validated private IR produced by the Topaz-authored compiler
//! kernel. Compiler-image embedding remains owned by `topaz_stage1_runtime`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use topaz_value::value::{
    Host, JsonValue, Value, array_spread_extend, builtin_byte_buffer_allocate,
    builtin_byte_buffer_from_bytes, builtin_csv_parse, builtin_csv_parse_with_header,
    builtin_csv_stringify, builtin_csv_stringify_with_header, builtin_fs_list,
    builtin_fs_read_bytes, builtin_fs_read_text, builtin_fs_write_bytes, builtin_fs_write_text,
    builtin_map_of, builtin_param_names, builtin_protocol_dispatch, builtin_set_of,
    builtin_toml_from_json, builtin_toml_parse, builtin_toml_stringify, builtin_toml_to_json,
    call_host_builtin, call_method, call_method_named, call_pure_builtin, call_resource_method,
    call_resource_method_named, call_spread_extend, canonical_abi_args_encode,
    canonical_abi_decode, canonical_abi_decode_args, canonical_abi_encode, condition_bool,
    for_items, make_range, member_value, nominal_declaration_identity, prepare_callback_hof,
    prepare_callback_key_collection, prepare_callback_map_hof, prepare_callback_map_update,
    prepare_callback_ok_or_else, prepare_callback_receiver_flat_map, prepare_callback_receiver_map,
    prepare_callback_retain, sorted_by_keys,
};
use topaz_value::{
    BinaryOp, Builtin, CALL_DEPTH_LIMIT, CallFuture, CallbackHofExecution, CallbackHofKind,
    CallbackHofStep, CallbackKeyCollection, CallbackKeyStep, CallbackMapHofExecution,
    CallbackMapHofKind, CallbackMapHofStep, CallbackMapUpdateStep, CallbackOkOrElseStep,
    CallbackReceiverMapStep, CallbackRetainExecution, CallbackRetainStep, FileId,
    ReceiverBuiltinRoute, RtCx, RtError, Span, TpzCall, UnaryOp, bind_builtin_named_args,
    bind_named_arg_slots, no_member_fault, receiver_builtin, receiver_builtin_by_kind,
    recursion_fault,
};

mod diagnostic;
pub(crate) mod program {
    pub(crate) mod decode_compact;
    pub(crate) mod decode_json;
    pub(crate) mod model;
    pub(crate) mod validate;
}
pub(crate) mod runtime {
    pub(crate) mod call;
    pub(crate) mod environment;
    pub(crate) mod evaluate;
    pub(crate) mod flow;
    pub(crate) mod machine;
    pub(crate) mod model;
    pub(crate) mod nominal;
}
mod wire;

pub use diagnostic::decode_runtime_diagnostic;
pub use program::decode_compact::parse_embedded_program;
pub use program::model::Program;
pub use wire::{
    execute_compiler, execute_compiler_program, execute_compiler_program_with_facts,
    execute_product_export, execute_product_export_in_place,
    execute_product_export_in_place_with_facts, execute_product_export_in_place_with_host_facts,
    execute_product_program, execute_product_program_with_facts,
    execute_product_program_with_facts_and_input,
    execute_product_program_with_host_facts_and_input,
};

const FIXED_POINT_PAYLOAD_SCHEMA: &str = "topaz.compiler.fixed-point-ir-payload/v1";
pub const TARGET_ADAPTER_FACTS_SCHEMA: &str = "topaz.self-target-adapter-facts/v1";
const PROPAGATE_SIGNAL: &str = "topaz.stage1.runtime.propagate";
const RETURN_SIGNAL: &str = "topaz.stage1.runtime.return";
const BREAK_SIGNAL: &str = "topaz.stage1.runtime.break";
const CONTINUE_SIGNAL: &str = "topaz.stage1.runtime.continue";
/// Native stack reserved for the boxed-future product executor. The executor
/// owns this bound because both target products and embedded compiler images
/// must be able to reach the shared Topaz call-depth limit without host abort.
pub const PRODUCT_RUNTIME_STACK_BYTES: usize = 64 * 1024 * 1024;
const RUNTIME_DIAGNOSTIC_PREFIX: &str = "topaz.runtime-diagnostic/v1\t";
// Fifty operations bound scheduler latency while amortizing cooperative-yield overhead.
const CONCURRENT_STEP_QUANTUM: usize = 50;

#[cfg(test)]
mod tests;
