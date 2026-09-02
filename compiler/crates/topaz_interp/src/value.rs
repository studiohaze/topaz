//! The interpreter's callable/template payloads (CDR-006 §3). The §2
//! data model — `Value`, equality, key canonicalization, rendering —
//! now lives in `topaz_value` (the shared core both engines carry);
//! re-exported here so the rest of `topaz_interp` keeps its paths.
//!
//! What stays interpreter-specific is the CONTENT behind the callable
//! ABI: an AST-backed closure (its params/body/captured environment/
//! defining source) and a tagged-template's parts. They implement
//! `TpzCall`/`TpzTemplate`; the interpreter recovers the concrete
//! closure by downcast to frame-execute it, and never drives the
//! async `call` (that is emitted code's path, landed here so the ABI
//! is final).

use std::any::Any;
use std::rc::Rc;

use topaz_diag::{FileId, Span};
use topaz_syntax::ast::{Block, Expr, Ident, LambdaParam, Param, Type};

use topaz_value::{CallFuture, RtCx, RtError, TpzCall, codes};

// §16 the tagged-template value + its builder moved to the shared core
// (CDR-006) so both engines render it identically.
pub use topaz_value::{
    Builtin, CALL_DEPTH_LIMIT, CallbackHofExecution, CallbackHofKind, CallbackHofPending,
    CallbackHofStep, CallbackKeyCollection, CallbackKeyPending, CallbackKeyStep,
    CallbackMapHofExecution, CallbackMapHofKind, CallbackMapHofPending, CallbackMapHofStep,
    CallbackMapUpdatePending, CallbackMapUpdateStep, CallbackOkOrElsePending, CallbackOkOrElseStep,
    CallbackReceiverMapPending, CallbackReceiverMapStep, CallbackRetainExecution,
    CallbackRetainPending, CallbackRetainStep, CmpError, ExternFunction, Key, OrderedMap,
    OrderedSet, ReceiverBuiltinRoute, Schema, SchemaAliasDecl, SchemaDecls, SchemaEnumDecl,
    SchemaNewtypeDecl, SchemaRecordDecl, Value, array_spread_extend, binary_value,
    bind_builtin_named_args, bind_named_arg_slots, builtin_json_decode, builtin_json_parse_as,
    builtin_map_of, builtin_protocol_dispatch, builtin_set_of, call_host_builtin, call_method,
    call_pure_builtin, call_resource_method, call_spread_extend, canonical_key, case_guard_bool,
    cmp_guard, condition_bool, decode_escapes, exact_args, for_items, index_slot, index_value,
    iterable_items, key_to_value, make_range, make_template, member_value, no_member_fault,
    nominal_record_field_required, nominal_spread_base_required, prepare_callback_hof,
    prepare_callback_key_collection, prepare_callback_map_hof, prepare_callback_map_update,
    prepare_callback_ok_or_else, prepare_callback_receiver_flat_map, prepare_callback_receiver_map,
    prepare_callback_retain, project_lispex_application_host_value, receiver_builtin,
    receiver_builtin_by_kind, record_update_base, record_update_merge, recursion_fault, render,
    rounding_mode_value, rounding_mode_variant, schema_of, short_circuit_lhs, sorted_by_keys,
    try_value, unary_value, update_fields_value, values_equal, walk_fields_value, wrap_optional,
};

/// A function value: a declared function or a lambda plus its
/// captured environment (CDR-003 §2/§3).
#[derive(Debug)]
pub struct ClosureData {
    pub name: Option<String>,
    pub params: ClosureParams,
    pub body: ClosureBody,
    pub env: crate::machine::EnvRef,
    /// The defining module's source text — spans inside `body` index
    /// into it; calls swap it in (§17 cross-module calls).
    pub src: Rc<str>,
    /// The function's own type parameters (empty for a lambda). A param or
    /// return type that NAMES one is generic, so its runtime boundary guard is
    /// skipped — the value is opaque at runtime. Type-param shadowing of a
    /// top-level alias is decided here, before alias resolution.
    pub type_params: Rc<[Ident]>,
    /// The declared return type (`None` when omitted, or for a lambda) —
    /// drives the return-boundary guard.
    pub return_type: Option<Type>,
}

impl TpzCall for ClosureData {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The interpreter never drives this — it recovers the concrete
    /// closure by downcast and steps it on the frame machine. The
    /// final async signature lands here (CDR-006 §3) so emitted code
    /// adds no trait redesign; this body is unreachable in the
    /// interpreter and yields an internal fault rather than panicking.
    fn call(&self, _cx: RtCx, _args: Vec<Value>) -> CallFuture {
        Box::pin(async {
            Err(RtError {
                code: codes::GUARD_UNIMPLEMENTED,
                message: "interpreter closures run on the frame machine, not the async call ABI"
                    .into(),
                span: Span::new(FileId(0), 0, 0),
            })
        })
    }

    // The interpreter drives arity through `apply_call`, not these — but
    // the trait requires them (emitted code's `call_value` uses them), so
    // they report the same fixed parameter count and names.
    fn arity(&self) -> usize {
        match &self.params {
            ClosureParams::Declared(p) => p.len(),
            ClosureParams::Lambda(p) => p.len(),
        }
    }

    fn param_name(&self, n: usize) -> Option<&str> {
        let span = match &self.params {
            ClosureParams::Declared(p) => p.get(n)?.name.span,
            ClosureParams::Lambda(p) => p.get(n)?.name.span,
        };
        Some(&self.src[span.lo as usize..span.hi as usize])
    }
}

#[derive(Debug, Clone)]
pub enum ClosureParams {
    Declared(Rc<[Param]>),
    Lambda(Rc<[LambdaParam]>),
}

#[derive(Debug, Clone)]
pub enum ClosureBody {
    Block(Rc<Block>),
    Expr(Rc<Expr>),
}

/// Recover the interpreter's concrete closure from a callable value's
/// trait object (CDR-006 §3 downcast bridge). `None` identifies a
/// shared-ABI callable such as an extern function rather than an
/// AST-backed closure.
pub fn as_closure(call: &Rc<dyn TpzCall>) -> Option<&ClosureData> {
    call.as_any().downcast_ref::<ClosureData>()
}
