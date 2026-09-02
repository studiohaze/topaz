use super::*;

/// Lexical environment: parent-chained scopes.
#[derive(Debug)]
pub struct Scope {
    /// Name lookup, redeclaration, and rebinding are the only consumers; lexical
    /// declaration order is represented by the AST and is never observed here.
    pub(super) vars: HashMap<String, BindingCell>,
    pub(super) parent: Option<EnvRef>,
    /// §14 deferred actions, run LIFO when this scope exits
    /// (including `return`/`?`/`break`/`continue` unwinds).
    pub(super) defers: Vec<DeferredAction>,
    /// Block-scoped type aliases (§5), pre-collected at block entry
    /// so forward references resolve (§6 runtime conformance).
    pub(super) aliases: BTreeMap<String, (Rc<[Ident]>, Rc<Type>)>,
}

#[derive(Debug)]
pub(super) struct BindingCell {
    pub(super) value: Value,
    pub(super) mutable: bool,
}

pub type EnvRef = Rc<RefCell<Scope>>;

pub(super) struct ClosureCallSlot<'a> {
    pub(super) name: String,
    pub(super) default: Option<&'a Expr>,
    pub(super) ty: Option<&'a Type>,
}

pub(super) struct PreparedClosureCall {
    pub(super) saved_env: EnvRef,
    pub(super) saved_src: Rc<str>,
    pub(super) return_guard: Option<(Type, Rc<str>)>,
}

pub(super) fn child_env(parent: &EnvRef) -> EnvRef {
    Rc::new(RefCell::new(Scope {
        vars: HashMap::new(),
        parent: Some(parent.clone()),
        defers: Vec::new(),
        aliases: BTreeMap::new(),
    }))
}

pub(super) fn lookup(env: &EnvRef, name: &str) -> Option<Value> {
    let scope = env.borrow();
    if let Some(cell) = scope.vars.get(name) {
        return Some(cell.value.clone());
    }
    let parent = scope.parent.clone()?;
    drop(scope);
    lookup(&parent, name)
}

/// Rebind through the chain; `Err` distinguishes "absent" from
/// "immutable" for the guard message.
pub(super) fn rebind(env: &EnvRef, name: &str, value: Value) -> Result<(), &'static str> {
    let mut scope = env.borrow_mut();
    if let Some(cell) = scope.vars.get_mut(name) {
        if !cell.mutable {
            return Err("immutable");
        }
        cell.value = value;
        return Ok(());
    }
    let parent = scope.parent.clone().ok_or("absent")?;
    drop(scope);
    rebind(&parent, name, value)
}

/// §12 container preservation with optional-layer flattening: an
/// `Option`-valued result stays one layer (flatMap); anything else
/// wraps into `Some` (map).
/// Whether an assignment target routes through optional access
/// (`?.`), which is conditional and not assignable (§4).
pub(super) fn target_has_optional(target: &Expr) -> bool {
    match &target.kind {
        ExprKind::OptionalAccess { .. } => true,
        ExprKind::Member { object, .. }
        | ExprKind::Index { object, .. }
        | ExprKind::Paren(object) => target_has_optional(object),
        _ => false,
    }
}

pub(super) fn is_mutable(env: &EnvRef, name: &str) -> bool {
    let scope = env.borrow();
    if let Some(cell) = scope.vars.get(name) {
        return cell.mutable;
    }
    let Some(parent) = scope.parent.clone() else {
        return false;
    };
    drop(scope);
    is_mutable(&parent, name)
}

// The runtime-stop identity (`RtError`), the shared `fault`
// constructor, and the TPZ4xxx/TPZ5xxx `codes` table live in the
// shared value core (CDR-006 §3) so fault identity cannot drift
// between the interpreter and emitted code. Re-exported here so
// `topaz_interp::machine::{RtError, codes, fault}` keep their paths.
