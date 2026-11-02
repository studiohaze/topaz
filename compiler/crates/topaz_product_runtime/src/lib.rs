//! Runtime for validated Topaz compiler IR.
//! Depends on shared values and syntax types, without a reference compiler.
//! Compiler-image embedding is supplied by the calling program.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use topaz_value::value::{
    Host, JsonValue, Value, array_spread_extend, builtin_map_of, builtin_param_names,
    builtin_protocol_dispatch, builtin_set_of, call_host_builtin, call_method, call_method_named,
    call_pure_builtin, call_resource_method, call_resource_method_named, call_spread_extend,
    canonical_abi_args_encode, canonical_abi_decode, canonical_abi_decode_args,
    canonical_abi_encode, condition_bool, for_items, make_range, nominal_declaration_identity,
    prepare_callback_hof, prepare_callback_key_collection, prepare_callback_map_hof,
    prepare_callback_map_update, prepare_callback_ok_or_else, prepare_callback_receiver_flat_map,
    prepare_callback_receiver_map, prepare_callback_retain, sorted_by_keys,
};
use topaz_value::{
    BinaryOp, Builtin, CALL_DEPTH_LIMIT, CallFuture, CallbackHofExecution, CallbackHofKind,
    CallbackHofStep, CallbackKeyCollection, CallbackKeyStep, CallbackMapHofExecution,
    CallbackMapHofKind, CallbackMapHofStep, CallbackMapUpdateStep, CallbackOkOrElseStep,
    CallbackReceiverMapStep, CallbackRetainExecution, CallbackRetainStep, FileId,
    ReceiverBuiltinRoute, RtCx, RtError, Span, TpzCall, UnaryOp, bind_builtin_named_args,
    bind_named_arg_slots, no_member_fault, receiver_builtin_by_kind, recursion_fault,
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
    pub(crate) mod json_schema;
    pub(crate) mod machine;
    pub(crate) mod model;
    pub(crate) mod nominal;
}
mod wire;

pub use diagnostic::decode_runtime_diagnostic;
pub use program::decode_compact::parse_embedded_program;
pub use program::decode_json::parse_compiler_program;
pub use program::model::Program;
pub use wire::{
    PreparedCompilerFacts, evaluate_product_program_with_host_facts_and_input, execute_compiler,
    execute_compiler_program, execute_compiler_program_with_facts,
    execute_compiler_program_with_prepared_facts, execute_product_export,
    execute_product_export_in_place, execute_product_export_in_place_with_facts,
    execute_product_export_in_place_with_host_facts, execute_product_program_with_facts,
    execute_product_program_with_facts_and_input,
    execute_product_program_with_host_facts_and_input, invoke_compiler_program_with_prepared_facts,
    prepare_compiler_facts,
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
const RUNTIME_DIAGNOSTIC_PREFIX: &str = "topaz.runtime-diagnostic/v2\t";
// Fifty operations bound scheduler latency while amortizing cooperative-yield overhead.
const CONCURRENT_STEP_QUANTUM: usize = 50;

