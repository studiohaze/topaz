//! `emit_native_checked` — the v5.4 NATIVE (monomorphized) emit backend.
//!
//! A SECOND, narrower backend that lowers HOT SCALAR islands to bare Rust
//! `i64`/`f64`/`bool`/`()` and routes their arithmetic/comparison through the
//! SHARED `topaz_value` checked-arith leaf (`int_add`/`int_div`/… —
//! `crate`-re-exported via `topaz_rt`), so a native-emitted program is
//! BYTE-IDENTICAL to the interpreter on the overflow/div0/`i64::MIN/-1`/
//! negative-exponent/float-rendering/fault-ordering axes (CDR-006 §2). For
//! anything it cannot guarantee byte-identical it REFUSES with a structured
//! `TPZ6002` ([`EmitError`] kind [`crate::EmitErrorKind::NativeDeclined`]) and
//! the caller falls back to the boxed backend — NEVER a diverging binary.
//!
//! SOUNDNESS (the design's top risk): a value lowers native ONLY when the
//! checker proved its local a concrete scalar — the native backend consumes the
//! checker-produced `TypedUnit` carried by the source-free `LoweredUnit`, NEVER
//! the checker itself, so the
//! `topaz_emit ↛ topaz_check` boundary holds (`topaz_check` does not appear in
//! `cargo tree -i topaz_check` under `topaz_emit`). The backend additionally
//! re-derives each scalar local's `MonoTy` locally and CROSS-CHECKS it against
//! the typed HIR: a disagreement (or a local the HIR did not record as that
//! scalar) is a refusal, so a native register can never rest on an untyped fact.
//!
//! ASYNC/CHECKPOINTS: the native entry and every native function are `async`.
//! A loop's back-edge `checkpoint().await` is ELIDED when the whole unit has no
//! `concurrent` (the `TypedUnit::contains_concurrent` fact) and KEPT otherwise.
//! Speed comes from the bare-scalar registers + inlined checked-arith leaf + the
//! elided per-iteration yield.
//!
//! ASYNC-NATIVE-FNS: an eligible scalar function is emitted as
//! `async fn f(cx: RtCx, __call_span: Span, args...) -> Result<scalar, RtError>`
//! whose first statement threads the SHARED recursion guard
//! (`__native_enter_call`). Native call sites use
//! `Box::pin(g(cx.clone(), <span>, args)).await?`, so self/mutual recursion and
//! long acyclic call chains compile while recursion-depth faults stay
//! byte-identical to interp+boxed.

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

use topaz_diag::{FileId, Span};
use topaz_hir::emission as ast;
use topaz_hir::emission::{
    AssignOp, BinaryOp, Block, CallArg, Expr, ExprKind, FunctionDecl, Pattern, PatternKind,
    Program, Stmt, StmtKind, StringPart, TypeKind, UnaryOp,
};
use topaz_hir::{LoweredUnit, MonoTy, TypedUnit, emission::LoweredText};
use topaz_value::decode_escapes;

use crate::{EmitError, emit_span, mangle, text};

mod boxed_boundary;
mod call;
mod context;
mod expression;
mod function;
mod hybrid;
mod math;
mod model;
mod statement;

use boxed_boundary::*;
use call::*;
use context::*;
use expression::*;
use function::*;
use hybrid::*;
use math::*;
use model::*;
use statement::*;

/// One deterministic top-level function decision in the opt-in native lowering
/// report. The `(module, path, name, span_lo, span_hi)` tuple is the full
/// identity; a name or byte offset is never used by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFunctionDecision {
    pub module: String,
    pub path: String,
    pub name: String,
    pub span_lo: u32,
    pub span_hi: u32,
    pub selected_backend: &'static str,
    pub selection_scope: &'static str,
    pub decline_reason: Option<&'static str>,
    pub decline_detail: Option<&'static str>,
}

/// The emitter-owned, command-independent facts for one native lowering
/// attempt. The CLI adds command/target/version metadata and serializes these
/// facts only when the user explicitly requests a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeAttemptDecision {
    pub selected_backend: &'static str,
    pub selection_scope: &'static str,
    pub decline_reason: Option<&'static str>,
    pub decline_detail: Option<&'static str>,
    pub entry_module: String,
    pub module_count: usize,
    pub contains_extern: bool,
    pub functions: Vec<NativeFunctionDecision>,
}

/// The selected Rust source and its machine-readable lowering decision. Whole
/// unit native remains the first choice; hybrid is considered only after that
/// attempt declines.
pub struct NativeLoweringOutcome {
    pub rust: String,
    pub decision: NativeAttemptDecision,
}

/// The source-free lowering vehicle consumed by the native backend. Its
/// `LoweredUnit` carries the diagnostics-clean `TypedUnit` representation facts,
/// so this crate remains free of checker and resolver dependencies.
pub struct NativeInput<'a> {
    /// The complete checked Lowered IR.
    pub unit: &'a LoweredUnit,
}

/// Describe an already-computed whole-unit native attempt without running the
/// emitter a second time. Report generation is therefore observational: it
/// cannot influence selection or emitted bytes.
pub fn describe_native_attempt(
    input: &NativeInput<'_>,
    attempt: &Result<String, EmitError>,
) -> NativeAttemptDecision {
    let (selected_backend, decline_reason, decline_detail) = match attempt {
        Ok(_) => ("native", None, None),
        Err(error) => {
            let (reason, detail) = stable_decline(error);
            ("boxed", Some(reason), detail)
        }
    };
    let mut functions = input
        .unit
        .modules
        .iter()
        .flat_map(|module| {
            let src = &module.text;
            module.program.items.iter().filter_map(move |statement| {
                let declaration = top_level_function(statement)?;
                Some(NativeFunctionDecision {
                    module: module.identity.clone(),
                    path: module.path.clone(),
                    name: text(src, declaration.name.span).to_string(),
                    span_lo: statement.span.lo,
                    span_hi: statement.span.hi,
                    selected_backend,
                    selection_scope: "unit",
                    decline_reason,
                    decline_detail,
                })
            })
        })
        .collect::<Vec<_>>();
    functions.sort_by(|left, right| {
        (
            left.module.as_str(),
            left.path.as_str(),
            left.span_lo,
            left.span_hi,
            left.name.as_str(),
        )
            .cmp(&(
                right.module.as_str(),
                right.path.as_str(),
                right.span_lo,
                right.span_hi,
                right.name.as_str(),
            ))
    });
    NativeAttemptDecision {
        selected_backend,
        selection_scope: "unit",
        decline_reason,
        decline_detail,
        entry_module: input
            .unit
            .modules
            .iter()
            .find(|module| module.is_entry)
            .map(|module| module.identity.clone())
            .unwrap_or_default(),
        module_count: input.unit.modules.len(),
        contains_extern: input.unit.modules.iter().any(|module| module.is_extern),
        functions,
    }
}

/// Select native, bounded hybrid, or boxed Rust in one shared emitter call.
/// The already-computed whole-unit attempt is never repeated, and report facts
/// describe the exact source that is returned.
pub fn emit_native_or_hybrid(input: &NativeInput<'_>) -> Result<NativeLoweringOutcome, EmitError> {
    let whole_error = match emit_native_checked(input) {
        Ok(rust) => {
            let decision = describe_native_attempt(input, &Ok(String::new()));
            return Ok(NativeLoweringOutcome { rust, decision });
        }
        Err(error) => error,
    };
    let hybrid = build_hybrid_plan(input);
    let selected_hybrid = hybrid.plan.has_closures();
    let mut decision = describe_native_attempt(input, &Err(whole_error));
    if selected_hybrid {
        decision.selected_backend = "hybrid-native";
        decision.selection_scope = "function";
        decision.decline_reason = None;
        decision.decline_detail = None;
    }
    for function in &mut decision.functions {
        match hybrid
            .decisions
            .get(&function.module, function.span_lo, function.span_hi)
        {
            Some(HybridDisposition::Selected) => {
                function.selected_backend = "hybrid-native";
                function.selection_scope = "function";
                function.decline_reason = None;
                function.decline_detail = None;
            }
            Some(HybridDisposition::Declined { reason, detail }) => {
                function.selected_backend = "boxed";
                function.selection_scope = "function";
                function.decline_reason = Some(reason);
                function.decline_detail = detail;
            }
            None => {}
        }
    }
    let rust = if selected_hybrid {
        crate::emit_module_with_hybrid(input.unit, hybrid.plan)?
    } else {
        crate::emit_module(input.unit)?
    };
    Ok(NativeLoweringOutcome { rust, decision })
}

/// Emit the native-checked CRATE source for a clean, single-module unit, or a
/// structured [`EmitError`] (`TPZ6002`/`TPZ6001`) the caller falls back from.
///
/// The envelope matches the boxed backend's `emit_module` (a `run_with_host`
/// over `block_on(entry(cx))`), so a native program is hostable identically and
/// the difftest harness can drive it the same way.
pub fn emit_native_checked(input: &NativeInput<'_>) -> Result<String, EmitError> {
    Ok(format!(
        "#![forbid(unsafe_code)]\n{}",
        emit_native_items(input)?
    ))
}

/// As [`emit_native_checked`] but WITHOUT the crate-level inner attribute, so the
/// difftest harness can `include!` each program inside its own `mod` (CDR-006 §7
/// — exactly the `emit_module`/`emit_unit` split the boxed backend uses).
pub fn emit_native_items(input: &NativeInput<'_>) -> Result<String, EmitError> {
    if input.unit.modules.iter().any(|m| m.is_extern) {
        return Err(decline("an extern unit"));
    }
    let Some(typed_hir) = input.unit.typed.as_ref() else {
        // No typed HIR = the unit did not check clean. Native rests only on a
        // sound check; refuse (the caller's checked build already reported the
        // diagnostics, and `--unchecked` never reaches here).
        return Err(decline("a unit without a clean type check"));
    };
    // SINGLE-MODULE only this slice, except the virtual `std.math` module when
    // imported as a no-op scalar namespace. Other import records are a boxed-value
    // concern (records of exports) the native island does not model.
    if input
        .unit
        .modules
        .iter()
        .any(|module| !module.is_entry && module.identity != "std.math")
    {
        return Err(decline("a multi-module unit"));
    }
    if input.unit.explicit_main_span().is_some() {
        return Err(decline("an explicit main entrypoint"));
    }
    let entry = input
        .unit
        .modules
        .iter()
        .find(|m| m.is_entry)
        .ok_or_else(EmitError::no_entry)?;
    let src = &entry.text;

    let hir_locals = TypedLocalIndex::from_typed_hir(typed_hir);
    let (byte_record_params, byte_projections) = byte_facts_for_module(typed_hir, &entry.identity);
    let mut ctx = Ctx {
        src,
        hir_locals: &hir_locals,
        current_function: None,
        byte_record_params: &byte_record_params,
        byte_projections: &byte_projections,
        fns: Cow::Owned(HashMap::new()),
        generic_fns: HashMap::new(),
        generic_specs: GenericFunctionIndex::default(),
        fn_defs: String::new(),
        // Elide loop checkpoints IFF the whole unit has no `concurrent` (the
        // conservative typed-HIR fact). Safe: see `Ctx::elide_checkpoints`.
        elide_checkpoints: !typed_hir.contains_concurrent,
        math_namespaces: Vec::new(),
        hybrid: false,
    };

    let body = emit_entry(&entry.program, &mut ctx)?;
    Ok(format!(
        "use std::rc::Rc;\n\
         use topaz_rt::*;\n\
         \n\
         /// The hostable entry (CDR-006 §4), NATIVE backend: bare-scalar islands\n\
         /// over the shared checked-arith leaf, boxed only at the boundary.\n\
         pub const TOPAZ_EXPLICIT_MAIN: bool = false;\n\
         \n\
         pub fn run_with_host(host: Rc<dyn Host>) -> RunOutcome {{\n\
         \x20   let cx = RtCx::new(host);\n\
         \x20   match block_on(entry(cx)) {{\n\
         \x20       Ok(value) => RunOutcome::Completed(value),\n\
         \x20       Err(error) => RunOutcome::Faulted(error),\n\
         \x20   }}\n\
         }}\n\
         \n\
         pub fn run_with_host_and_input(host: Rc<dyn Host>, args: Vec<String>, stdin: String) -> RunOutcome {{\n\
         \x20   let _ = (args, stdin);\n\
         \x20   run_with_host(host)\n\
         }}\n\
         \n\
         {fns}\
         async fn entry(cx: RtCx) -> Result<Value, RtError> {{\n\
         \x20   let _ = &cx;\n\
         {body}\
         }}\n",
        fns = ctx.fn_defs,
    ))
}

#[cfg(test)]
mod tests;
