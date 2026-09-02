//! The shared runtime value (CDR-006 §3): ONE concrete `Value` both
//! the interpreter and emitted code carry. Records are immutable
//! values; arrays/maps/sets are shared mutable references with
//! insertion order; equality implements SPEC §2 exactly. Map keys
//! and set elements are canonicalized deep snapshots, so mutating a
//! source aggregate can never corrupt lookup or order.
//!
//! The callable and template payloads are trait objects
//! (`Rc<dyn TpzCall>` / `Rc<dyn TpzTemplate>`): the interpreter
//! instantiates them with AST-backed closures, emitted code with
//! compiled functions — so the data semantics here are engine-neutral
//! while the behavior lives at the leaves.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use topaz_diag::Span;
use topaz_syntax::ast::{self, BinaryOp, Expr, ExprKind, StringPart, TypeKind, UnaryOp};

use crate::lispex_application::{LispexApplicationOperation, builtin_lispex_application};
use crate::{RtError, codes, fault};

mod bigint;
mod builtin_catalog;
mod builtins;
mod callbacks;
mod collections;
mod compare;
mod decimal;
mod extern_replay;
mod guards;
mod json;
mod members;
mod model;
mod operators;
mod regex;
mod render;
mod runtime;

pub use builtin_catalog::*;
pub use builtins::*;
pub use callbacks::*;
pub use collections::*;
pub use compare::*;
pub use extern_replay::*;
pub use guards::*;
pub use json::*;
pub use members::*;
pub use model::*;
pub use operators::*;
pub use regex::*;
pub use render::*;
pub use runtime::*;

#[cfg(test)]
mod tests;
