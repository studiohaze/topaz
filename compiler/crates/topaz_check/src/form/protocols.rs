use super::*;

pub(super) fn catalog_contains_conformance(
    catalog: &ConformanceCatalog,
    protocol: &str,
    type_id: &str,
) -> bool {
    catalog
        .get(protocol)
        .is_some_and(|types| types.contains(type_id))
}

pub(super) fn catalog_insert_conformance(
    catalog: &mut ConformanceCatalog,
    protocol: String,
    type_id: String,
) {
    catalog.entry(protocol).or_default().insert(type_id);
}

impl<'a> Former<'a> {
    /// §4 (v5.4) collects every PROTOCOL declaration into the `protocols` table, and
    /// PREDECLARES the builtin protocols `Show`/`Eq`/`Order`/`JSON` (so
    /// `Show.show(x)` works on a `derives Show` type with no `protocol Show { … }`
    /// in source, and `T: JSON` can gate JSON-safe generic APIs). Run BEFORE
    /// `collect_methods` (a manual `impl Foo<T>` validates against the protocol
    /// surface) and `collect_derives`. Each protocol method signature forms over the
    /// protocol's conforming type variable `T` = `Type::Var(0)` (every `Self`/`<T>`
    /// mention is `Var(0)`), so a `P.m(x)` call substitutes the receiver's concrete
    /// type. A user protocol whose name collides with a builtin protocol, an alias, or
    /// a nominal type is rejected (TPZ5022); a duplicate protocol is a redeclaration
    /// (TPZ5008); a duplicate method name within one protocol is a redeclaration.
    pub(super) fn collect_protocols(&mut self, items: &'a [ast::Stmt]) {
        // Predeclare the builtin protocols. Their signatures use `Type::Var(0)` for
        // the conforming type `T`; a call substitutes the receiver's concrete type.
        // Show.show(value: T) -> string ; Eq.equals(a: T, b: T) -> bool ;
        // Order.compare(a: T, b: T) -> int. These are the surfaces the value.rs leaves
        // (render / values_equal / values_compare) implement for a derived conformance.
        let t = Type::Var(0);
        let sig1 = |ret: Type| ProtocolMethodSig {
            params: vec![t.clone()],
            variadic: None,
            ret,
            required: 1,
            names: vec!["value".to_string()],
            defaulted: vec![false],
        };
        let sig2 = |ret: Type| ProtocolMethodSig {
            params: vec![t.clone(), t.clone()],
            variadic: None,
            ret,
            required: 2,
            names: vec!["a".to_string(), "b".to_string()],
            defaulted: vec![false, false],
        };
        let mut show = std::collections::BTreeMap::new();
        show.insert(
            builtins::SHOW_PROTOCOL_SURFACE.1.to_string(),
            sig1(Type::Prim(Prim::String)),
        );
        let mut eq = std::collections::BTreeMap::new();
        eq.insert(
            builtins::EQ_PROTOCOL_SURFACE.1.to_string(),
            sig2(Type::Prim(Prim::Bool)),
        );
        let mut order = std::collections::BTreeMap::new();
        order.insert(
            builtins::ORDER_PROTOCOL_SURFACE.1.to_string(),
            sig2(Type::Prim(Prim::Int)),
        );
        self.protocols.insert(
            builtins::SHOW_PROTOCOL_SURFACE.0.to_string(),
            ProtocolInfo { methods: show },
        );
        self.protocols.insert(
            builtins::EQ_PROTOCOL_SURFACE.0.to_string(),
            ProtocolInfo { methods: eq },
        );
        self.protocols.insert(
            builtins::ORDER_PROTOCOL_SURFACE.0.to_string(),
            ProtocolInfo { methods: order },
        );
        // JSON is derive-only in v5.4: it has no `JSON.method(x)` protocol dispatch
        // surface because `JSON.stringify`/`parseAs` are already builtin namespace
        // calls. The protocol name exists so `T: JSON` bounds can require a derived
        // JSON-roundtrippable nominal.
        self.protocols.insert(
            "JSON".to_string(),
            ProtocolInfo {
                methods: std::collections::BTreeMap::new(),
            },
        );

        // User `protocol Foo { … }` declarations.
        for stmt in items {
            let inner = match &stmt.kind {
                ast::StmtKind::Export(inner) => {
                    if let ast::StmtKind::Protocol(decl) = &inner.kind {
                        self.error(
                            codes::MALFORMED_TYPE,
                            "user protocols are module-local in v5.6; `export protocol` is not supported"
                                .to_string(),
                            decl.name.span,
                        );
                        continue;
                    }
                    &**inner
                }
                _ => stmt,
            };
            let ast::StmtKind::Protocol(decl) = &inner.kind else {
                continue;
            };
            let name = self.text(decl.name.span).to_string();
            // Name hygiene: a protocol name may not collide with a builtin protocol,
            // a nominal type, or an alias.
            if matches!(name.as_str(), "Show" | "Eq" | "Order" | "JSON") {
                self.error(
                    codes::REDECLARE,
                    format!("`{name}` is a builtin protocol and cannot be redeclared"),
                    decl.name.span,
                );
                continue;
            }
            if self.protocols.contains_key(&name) {
                self.error(
                    codes::REDECLARE,
                    format!("protocol `{name}` is already declared"),
                    decl.name.span,
                );
                continue;
            }
            if self.alias_lookup(&name)
                || self.is_record(&name)
                || self.is_enum(&name)
                || self.is_newtype(&name)
            {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!("`{name}` is already a type and cannot also be a protocol"),
                    decl.name.span,
                );
                continue;
            }
            if decl.type_params.len() > 1 {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!(
                        "protocol `{name}` takes at most one conforming-type parameter; found {}",
                        decl.type_params.len()
                    ),
                    decl.name.span,
                );
                continue;
            }
            // The conforming type stand-in: `Self` (no params) OR the single `<T>`.
            // Both map to `Type::Var(0)` in the formed signatures.
            let mut tenv: HashMap<&'a str, Type> = HashMap::new();
            // `Self` always denotes the conforming type; `<T>` (when present) too.
            tenv.insert("Self", Type::Var(0));
            if let Some(tp) = decl.type_params.first() {
                tenv.insert(self.text(tp.span), Type::Var(0));
            }
            let mut methods: std::collections::BTreeMap<String, ProtocolMethodSig> =
                std::collections::BTreeMap::new();
            for m in &decl.methods {
                let mname = self.text(m.name.span).to_string();
                if methods.contains_key(&mname) {
                    self.error(
                        codes::REDECLARE,
                        format!("method `{mname}` is already declared in protocol `{name}`"),
                        m.name.span,
                    );
                    continue;
                }
                if !m.type_params.is_empty() {
                    self.error(
                        codes::MALFORMED_TYPE,
                        format!("protocol method `{name}.{mname}` cannot be generic"),
                        m.name.span,
                    );
                    continue;
                }
                let Some(first) = m.params.first() else {
                    self.error(
                        codes::MALFORMED_TYPE,
                        format!(
                            "protocol method `{name}.{mname}` must take the conforming value as its first parameter"
                        ),
                        m.name.span,
                    );
                    continue;
                };
                if m.return_type.is_none() {
                    self.error(
                        codes::MALFORMED_TYPE,
                        format!(
                            "protocol method `{name}.{mname}` requires an explicit return type (use `-> ()` for unit)"
                        ),
                        m.name.span,
                    );
                    continue;
                }
                if let Some(param) = m.params.iter().find(|param| param.variadic) {
                    self.error(
                        codes::MALFORMED_TYPE,
                        format!("protocol method `{name}.{mname}` cannot be variadic"),
                        param.span,
                    );
                    continue;
                }
                if let Some(param) = m.params.iter().find(|param| param.default.is_some()) {
                    self.error(
                        codes::MALFORMED_TYPE,
                        format!(
                            "protocol method `{name}.{mname}` cannot declare parameter defaults"
                        ),
                        param.span,
                    );
                    continue;
                }
                let mut params = Vec::new();
                let variadic = None;
                let mut names = Vec::new();
                for p in &m.params {
                    let ty = self.form_signature(&p.ty, &tenv);
                    names.push(self.text(p.name.span).to_string());
                    params.push(ty);
                }
                if params.first() != Some(&Type::Var(0)) {
                    self.error(
                        codes::MALFORMED_TYPE,
                        format!(
                            "protocol method `{name}.{mname}` must use `Self` or the protocol's type parameter as its first parameter type"
                        ),
                        first.ty.span,
                    );
                    continue;
                }
                let ret = self.form_signature(
                    m.return_type
                        .as_ref()
                        .expect("explicit return checked above"),
                    &tenv,
                );
                let required = params.len();
                let defaulted = vec![false; params.len()];
                methods.insert(
                    mname,
                    ProtocolMethodSig {
                        params,
                        variadic,
                        ret,
                        required,
                        names,
                        defaulted,
                    },
                );
            }
            self.protocols.insert(name, ProtocolInfo { methods });
        }
    }

    /// §4 (v5.4) collects every top-level `impl Type { … }` block (top level only)
    /// into the method table, AFTER the nominal tables form (so a method's
    /// parameter/return types may name any record/enum/newtype). Per impl + method:
    ///
    /// - COHERENCE / orphan rule (TPZ5022): the receiver `Type` must be a declared
    ///   OWN-module nominal (record/enum/newtype). An `impl` on a builtin, a
    ///   structural type, or an undeclared name is rejected and SKIPPED.
    /// - `self` (TPZ5022): the first parameter must be named `self`; a method with
    ///   no parameters, or whose first parameter is not `self`, is rejected.
    /// - DUPLICATE method (TPZ5008): two methods of the same name on one type.
    /// - FIELD/METHOD COLLISION (TPZ5022): a method whose name equals a record FIELD
    ///   is rejected — so field-vs-method precedence never silently shadows (the
    ///   documented safe rule). Methods whose names collide with BUILTIN members
    ///   (e.g. `value` on a newtype, `map` on a record) are also rejected, keeping
    ///   method lookup unambiguous across check/run/build.
    ///
    /// Generic receiver types/methods, annotated/defaulted/variadic receiver slots,
    /// protocol/derive methods, and methods that don't take a bare first `self` are
    /// out of this slice.
    pub(super) fn collect_methods(&mut self, items: &'a [ast::Stmt]) {
        let empty_env: HashMap<&'a str, Type> = HashMap::new();
        for stmt in items {
            let inner = match &stmt.kind {
                ast::StmtKind::Export(inner) => {
                    if let ast::StmtKind::Impl(decl) = &inner.kind {
                        self.error(
                            codes::MALFORMED_TYPE,
                            "an `impl` block cannot be exported; mark individual methods with `export`"
                                .to_string(),
                            decl.name.span,
                        );
                        continue;
                    }
                    &**inner
                }
                _ => stmt,
            };
            let ast::StmtKind::Impl(decl) = &inner.kind else {
                continue;
            };
            // §4 PROTOCOL impl `impl Show<User> { … }`: `decl.name` is the PROTOCOL,
            // `decl.target` the conforming type. Route to the protocol-impl collector
            // (free-function methods, orphan rule, conformance registration).
            if decl.target.is_some() {
                self.collect_protocol_impl(decl);
                continue;
            }
            let type_id = self.text(decl.name.span).to_string();
            // COHERENCE: the receiver must be an own-module nominal type.
            let is_nominal = self.own_nominals.contains(&type_id);
            if !is_nominal {
                let what = if reserved_type_name_kind(&type_id).is_some() {
                    "a builtin type"
                } else {
                    "not a declared record/enum/newtype in this module"
                };
                self.error(
                    codes::MALFORMED_TYPE,
                    format!(
                        "cannot define methods on `{type_id}` ({what}); `impl` is only allowed on an own-module record, enum, or newtype"
                    ),
                    decl.name.span,
                );
                continue;
            }
            // The inherent-impl grammar has no binder for a nominal's type
            // parameters. Treating `impl Box` as an erased implementation for every
            // `Box<T>` would lose the receiver's invariant argument and make `self`
            // ill-typed. Keep generic nominal impls closed until a later ticket
            // defines the binder and substitution model.
            if self.nominal_is_generic(&type_id) {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!(
                        "cannot define receiver methods on generic nominal `{type_id}` yet; generic `impl` binders are not defined"
                    ),
                    decl.name.span,
                );
                continue;
            }
            for m in &decl.methods {
                let mname = self.text(m.decl.name.span).to_string();
                // The receiver is exactly the bare, immutable, by-value first token
                // `self`. An annotation/default/variadic marker would introduce a
                // second competing type or calling convention, so reject it rather
                // than continuing to ignore it.
                let Some(first) = m.decl.params.first() else {
                    self.error(
                        codes::MALFORMED_TYPE,
                        format!(
                            "method `{mname}` on `{type_id}` must take `self` as its first parameter"
                        ),
                        m.decl.name.span,
                    );
                    continue;
                };
                if !self.is_bare_self_param(first) {
                    self.error(
                        codes::MALFORMED_TYPE,
                        format!(
                            "method `{mname}` on `{type_id}` must take bare `self` as its first parameter (no type annotation, default, or variadic marker)"
                        ),
                        first.span,
                    );
                    continue;
                }
                // Generic methods are not in this slice.
                if !m.decl.type_params.is_empty() {
                    self.error(
                        codes::MALFORMED_TYPE,
                        format!("generic methods are not supported yet (method `{mname}`)"),
                        m.decl.name.span,
                    );
                    continue;
                }
                // A method name may not collide with a BUILTIN member name — that
                // would shadow `arr.map`/`id.value`/`s.length`/… at a call site, which
                // the dispatch + run≡build cannot keep coherent. Reject + skip.
                if builtins::is_reserved_receiver_member_name(&mname) {
                    self.error(
                        codes::MALFORMED_TYPE,
                        format!(
                            "method name `{mname}` on `{type_id}` collides with a builtin member; choose another name"
                        ),
                        m.decl.name.span,
                    );
                    continue;
                }
                // DUPLICATE method.
                if self
                    .methods
                    .get(&type_id)
                    .is_some_and(|methods| methods.contains_key(&mname))
                {
                    self.error(
                        codes::REDECLARE,
                        format!("method `{mname}` is already defined for `{type_id}`"),
                        m.decl.name.span,
                    );
                    continue;
                }
                // FIELD/METHOD collision (records only — enums/newtypes have no
                // user fields). A method named like a field is rejected so the
                // field-first precedence rule never silently shadows the method.
                if let Some(rec) = self.records.get(&type_id)
                    && rec.fields.iter().any(|f| f.name == mname)
                {
                    self.error(
                        codes::MALFORMED_TYPE,
                        format!(
                            "method `{mname}` on `{type_id}` collides with a field of the same name; rename the method (a field shadows a method)"
                        ),
                        m.decl.name.span,
                    );
                    continue;
                }
                // Form the NON-`self` parameter signature (self is the receiver).
                let mut params: Vec<Type> = Vec::new();
                let mut variadic: Option<Type> = None;
                let mut required = 0usize;
                let mut names: Vec<String> = Vec::new();
                let mut defaulted: Vec<bool> = Vec::new();
                for p in m.decl.params.iter().skip(1) {
                    let ty = self.form_signature(&p.ty, &empty_env);
                    if p.variadic {
                        variadic = Some(ty);
                    } else {
                        if p.default.is_none() {
                            required += 1;
                        }
                        names.push(self.text(p.name.span).to_string());
                        defaulted.push(p.default.is_some());
                        params.push(ty);
                    }
                }
                let ret = match &m.decl.return_type {
                    Some(r) => self.form_signature(r, &empty_env),
                    None => Type::Unknown,
                };
                self.methods.entry(type_id.clone()).or_default().insert(
                    mname,
                    InherentMethodInfo {
                        signature: MethodInfo {
                            params,
                            variadic,
                            ret,
                            required,
                            names,
                            defaulted,
                        },
                        dispatch_id: None,
                    },
                );
                self.accepted_receiver_methods.insert(m.decl.name.span);
            }
        }
    }

    /// §4.2 (v5.4) collects ONE manual `impl Protocol<Type> { … }` block: validates
    /// the protocol exists, the conforming type is a declared own-module nominal, the
    /// ORPHAN rule (TPZ5520 — protocol OR type must be own-module), the
    /// DOUBLE-CONFORMANCE conflict (TPZ5521 — the type may not already conform via
    /// `derives` or another `impl`), and that the impl supplies exactly the protocol's
    /// methods with matching arity. On success, registers `(protocol, type_id) ∈
    /// conformances` (NOT `derived_conformances`) + each method body's CONCRETE
    /// signature in `protocol_methods`. The method bodies are FREE functions (no
    /// `self`); `check_impl` type-checks them in the expression pass.
    pub(super) fn collect_protocol_impl(&mut self, decl: &'a ast::ImplDecl) {
        let protocol = self.text(decl.name.span).to_string();
        let target = decl.target.expect("protocol impl has a target");
        let type_id = self.text(target.span).to_string();

        // Every protocol-implementation method is still a source function. Form
        // its complete signature before protocol/coherence admission so an unknown
        // method or otherwise rejected impl cannot hide a malformed annotation.
        // The body pass sees the recorded spans and only performs its rigid
        // reprojection, matching the self-host predeclaration boundary.
        let prepared_methods = decl
            .methods
            .iter()
            .map(|method| {
                let declaration = &method.decl;
                let mut env = HashMap::new();
                for (index, parameter) in declaration.type_params.iter().enumerate() {
                    env.insert(self.text(parameter.span), Type::Var(index as u32));
                }
                let params = declaration
                    .params
                    .iter()
                    .map(|parameter| self.form_signature(&parameter.ty, &env))
                    .collect::<Vec<_>>();
                let names = declaration
                    .params
                    .iter()
                    .map(|parameter| self.text(parameter.name.span).to_string())
                    .collect::<Vec<_>>();
                let ret = declaration
                    .return_type
                    .as_ref()
                    .map(|result| self.form_signature(result, &env));
                (params, names, ret)
            })
            .collect::<Vec<_>>();
        let Some(protocol_info) = self.protocols.get(&protocol).cloned() else {
            self.error(
                codes::MALFORMED_TYPE,
                format!("unknown protocol `{protocol}` in `impl {protocol}<{type_id}>`"),
                decl.name.span,
            );
            return;
        };
        // The conforming type must be a declared own-module nominal.
        let type_is_own = self.own_nominals.contains(&type_id);
        if protocol == "JSON" {
            self.error(
                codes::MALFORMED_TYPE,
                "`JSON` conformance is derive-only; use `derives JSON`".to_string(),
                decl.name.span,
            );
            return;
        }
        // A protocol is "own-module" when it is a USER protocol (a `protocol Foo { … }`
        // declaration here). The builtins `Show`/`Eq`/`Order`/`JSON` are FOREIGN
        // (stdlib/derive surfaces).
        let protocol_is_own = !matches!(protocol.as_str(), "Show" | "Eq" | "Order" | "JSON");
        // ORPHAN rule (§4.2): the protocol OR the type must be own-module. In this
        // slice cross-module impls don't exist, so "foreign type" = not-a-nominal
        // (a builtin like `int`/`string`). A foreign protocol (a builtin) on a
        // foreign type (a builtin) is the orphan case.
        if !type_is_own {
            if protocol_is_own {
                // Own protocol, foreign type: still need the type to be SOMETHING we
                // can dispatch on. A builtin type has no nominal id at runtime, so we
                // cannot register a conformance for it — reject as unsupported.
                self.error(
                    codes::ORPHAN_IMPL,
                    format!(
                        "cannot implement `{protocol}` for `{type_id}`: a protocol impl's conforming type must be an own-module record/enum/newtype"
                    ),
                    target.span,
                );
            } else {
                // Foreign protocol AND foreign type → the classic orphan impl.
                self.error(
                    codes::ORPHAN_IMPL,
                    format!(
                        "cannot implement foreign protocol `{protocol}` for foreign type `{type_id}` (orphan rule §4.2): the protocol or the type must be defined in this module"
                    ),
                    target.span,
                );
            }
            return;
        }
        if self.nominal_is_generic(&type_id) {
            self.error(
                codes::MALFORMED_TYPE,
                format!(
                    "cannot implement protocol `{protocol}` for generic nominal `{type_id}` yet; generic conformance binders are not defined"
                ),
                target.span,
            );
            return;
        }
        // DOUBLE-CONFORMANCE conflict (§4.2): the type must not already conform.
        if catalog_contains_conformance(&self.conformances, &protocol, &type_id) {
            let how =
                if catalog_contains_conformance(&self.derived_conformances, &protocol, &type_id) {
                    "via `derives`"
                } else {
                    "by a previous `impl`"
                };
            self.error(
                codes::DUPLICATE_IMPL,
                format!("`{type_id}` already conforms to `{protocol}` {how}; a conformance must be unique"),
                target.span,
            );
            return;
        }

        let receiver = if self.is_record(&type_id) {
            Type::NominalRecord {
                base: type_id.clone(),
                args: Vec::new(),
            }
        } else if self.is_enum(&type_id) {
            Type::Enum {
                base: type_id.clone(),
                args: Vec::new(),
            }
        } else {
            Type::Newtype {
                base: type_id.clone(),
                args: Vec::new(),
            }
        };
        let required_methods: Vec<String> = protocol_info.methods.keys().cloned().collect();
        let mut seen: HashSet<String> = HashSet::new();
        let mut accepted: Vec<(String, MethodInfo)> = Vec::new();
        let mut valid = true;
        for (m, (params, names, ret)) in decl.methods.iter().zip(prepared_methods) {
            let mname = self.text(m.decl.name.span).to_string();
            // The method must belong to the protocol.
            let Some(expected_sig) = protocol_info.methods.get(&mname) else {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!("protocol `{protocol}` has no method `{mname}`"),
                    m.decl.name.span,
                );
                valid = false;
                continue;
            };
            if !seen.insert(mname.clone()) {
                self.error(
                    codes::REDECLARE,
                    format!(
                        "method `{mname}` is implemented twice in `impl {protocol}<{type_id}>`"
                    ),
                    m.decl.name.span,
                );
                valid = false;
                continue;
            }
            if m.exported {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!(
                        "protocol implementation method `{protocol}.{mname}` is module-local and cannot be exported"
                    ),
                    m.span,
                );
                valid = false;
                continue;
            }
            if !m.decl.type_params.is_empty() {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!(
                        "protocol implementation method `{protocol}.{mname}` cannot be generic"
                    ),
                    m.decl.name.span,
                );
                valid = false;
                continue;
            }
            if m.decl.return_type.is_none() {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!(
                        "protocol implementation method `{protocol}.{mname}` requires an explicit return type (use `-> ()` for unit)"
                    ),
                    m.decl.name.span,
                );
                valid = false;
                continue;
            }
            if let Some(param) = m.decl.params.iter().find(|param| param.variadic) {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!(
                        "protocol implementation method `{protocol}.{mname}` cannot be variadic"
                    ),
                    param.span,
                );
                valid = false;
                continue;
            }
            if let Some(param) = m.decl.params.iter().find(|param| param.default.is_some()) {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!(
                        "protocol implementation method `{protocol}.{mname}` cannot declare parameter defaults"
                    ),
                    param.span,
                );
                valid = false;
                continue;
            }

            let ret = ret.expect("explicit return checked above");
            let expected_params: Vec<Type> = expected_sig
                .params
                .iter()
                .map(|ty| substitute(ty, std::slice::from_ref(&receiver)))
                .collect();
            let expected_ret = substitute(&expected_sig.ret, std::slice::from_ref(&receiver));
            if params != expected_params || ret != expected_ret {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!(
                        "protocol implementation method `{protocol}.{mname}` must match the declared signature exactly; expected ({}) -> `{expected_ret}`, found ({}) -> `{ret}`",
                        expected_params
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", "),
                        params
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    m.decl.name.span,
                );
                valid = false;
                continue;
            }
            accepted.push((
                mname,
                MethodInfo {
                    required: params.len(),
                    defaulted: vec![false; params.len()],
                    params,
                    variadic: None,
                    ret,
                    names,
                },
            ));
        }
        // Completeness: every protocol method must be implemented.
        for req in &required_methods {
            if !seen.contains(req) {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!("`impl {protocol}<{type_id}>` is missing method `{req}`"),
                    decl.name.span,
                );
                valid = false;
            }
        }
        if !valid {
            return;
        }
        catalog_insert_conformance(&mut self.conformances, protocol.clone(), type_id.clone());
        self.protocol_methods
            .entry(protocol)
            .or_default()
            .entry(type_id)
            .or_default()
            .extend(accepted);
    }

    /// §4 (v5.4) collects every record/enum `derives Eq, Order, Show, JSON` clause into
    /// the conformance table, run AFTER `collect_nominals`/`collect_methods` so the
    /// nominal field/payload types are formed (derivability needs them). This is
    /// CHECKER-ONLY bookkeeping — it AUTHORIZES the capability; the runtime eq/
    /// compare/render leaves (value.rs) are untouched and `==`/`<`/render keep
    /// working exactly as today. Per derive NAME in each clause:
    ///
    /// - The name must be a SUPPORTED, derivable protocol — `Eq`, `Order`,
    ///   `Show`, or `JSON`. Any other name is an UNKNOWN protocol (TPZ5530).
    /// - DERIVABILITY: `Show` is always derivable. `Eq`/`Order` require every
    ///   record field type (every enum payload type) to be COMPARABLE — reusing
    ///   `comparable_in` (the SAME predicate the `==`/`<` checker uses). A
    ///   function-typed (or otherwise non-comparable) field/payload makes the
    ///   `Eq`/`Order` derive ill-formed (TPZ5530), naming the offending member.
    ///   `JSON` requires every record field / enum payload type to be both
    ///   statically JSON-encodable and JSON-decodable, using the same predicates as
    ///   `JSON.stringify` and `JSON.parseAs`.
    /// - On success, `(protocol, type_id)` is inserted into `conformances`.
    ///
    /// NOTE on Order: the shared runtime `values_compare` leaf orders a nominal
    /// record by field declaration order and an enum by variant declaration order
    /// then payloads left-to-right. `derives Order` authorizes the existing
    /// `Order.compare` dispatch to that leaf; ordinary `<` keeps its own checker
    /// admission boundary.
    pub(super) fn collect_derives(&mut self, items: &'a [ast::Stmt]) {
        for stmt in items {
            let inner = match &stmt.kind {
                ast::StmtKind::Export(inner) => &**inner,
                _ => stmt,
            };
            match &inner.kind {
                ast::StmtKind::Record(decl) => {
                    let type_id = self.text(decl.name.span).to_string();
                    // Only process derives for a record that actually formed (an
                    // ill-formed/duplicate name was skipped in collect_nominals).
                    if !self.is_record(&type_id) {
                        continue;
                    }
                    if self.nominal_is_generic(&type_id) && !decl.derives.is_empty() {
                        for derive in &decl.derives {
                            let protocol = self.text(derive.span);
                            self.error(
                                codes::MALFORMED_TYPE,
                                format!(
                                    "cannot derive `{protocol}` for generic nominal `{type_id}` yet; conditional generic conformance is not defined"
                                ),
                                derive.span,
                            );
                        }
                        continue;
                    }
                    for d in &decl.derives {
                        self.collect_one_derive(&type_id, d, false);
                    }
                }
                ast::StmtKind::Enum(decl) => {
                    let type_id = self.text(decl.name.span).to_string();
                    if !self.is_enum(&type_id) {
                        continue;
                    }
                    if self.nominal_is_generic(&type_id) && !decl.derives.is_empty() {
                        for derive in &decl.derives {
                            let protocol = self.text(derive.span);
                            self.error(
                                codes::MALFORMED_TYPE,
                                format!(
                                    "cannot derive `{protocol}` for generic nominal `{type_id}` yet; conditional generic conformance is not defined"
                                ),
                                derive.span,
                            );
                        }
                        continue;
                    }
                    for d in &decl.derives {
                        self.collect_one_derive(&type_id, d, true);
                    }
                }
                _ => {}
            }
        }
    }

    /// Validates ONE `derives` name on a nominal `type_id` (record when
    /// `is_enum == false`, enum otherwise) and, on success, records the
    /// `(protocol, type_id)` conformance. See [`Self::collect_derives`].
    pub(super) fn collect_one_derive(&mut self, type_id: &str, name: &ast::Ident, is_enum: bool) {
        let proto = self.text(name.span).to_string();
        match proto.as_str() {
            "Show" => {
                // Always derivable: render works for every nominal value.
                self.record_derived(&proto, type_id, name.span);
            }
            "Eq" => {
                if let Some(bad) =
                    self.first_member_without_capability(type_id, is_enum, DeriveCapability::Eq)
                {
                    self.error(
                        codes::NOT_DERIVABLE,
                        format!(
                            "cannot derive `{proto}` for `{type_id}`: {bad} has a non-comparable type"
                        ),
                        name.span,
                    );
                } else {
                    self.record_derived(&proto, type_id, name.span);
                }
            }
            "Order" => {
                if let Some(bad) =
                    self.first_member_without_capability(type_id, is_enum, DeriveCapability::Order)
                {
                    self.error(
                        codes::NOT_DERIVABLE,
                        format!(
                            "cannot derive `Order` for `{type_id}`: {bad} is not totally orderable"
                        ),
                        name.span,
                    );
                } else {
                    self.record_derived(&proto, type_id, name.span);
                }
            }
            "JSON" => {
                if let Some(bad) =
                    self.first_member_without_capability(type_id, is_enum, DeriveCapability::Json)
                {
                    self.error(
                        codes::NOT_DERIVABLE,
                        format!(
                            "cannot derive `JSON` for `{type_id}`: {bad} cannot round-trip through JSON"
                        ),
                        name.span,
                    );
                } else {
                    self.record_derived(&proto, type_id, name.span);
                }
            }
            other => {
                self.error(
                    codes::NOT_DERIVABLE,
                    format!(
                        "unknown protocol `{other}` in `derives` for `{type_id}`; derivable protocols are `Eq`, `Order`, `Show`, `JSON`"
                    ),
                    name.span,
                );
            }
        }
    }

    /// §4.2 (v5.4) records a DERIVED conformance `(protocol, type_id)`, rejecting a
    /// DOUBLE conformance (TPZ5521) when the type ALREADY conforms — by a manual
    /// `impl` (collected before derives) OR a duplicate `derives` of the same
    /// protocol. A unique derived conformance is added to both `conformances` and
    /// `derived_conformances` (so the call-site dispatch routes to the value leaf).
    pub(super) fn record_derived(&mut self, proto: &str, type_id: &str, span: Span) {
        if catalog_contains_conformance(&self.conformances, proto, type_id) {
            let how = if catalog_contains_conformance(&self.derived_conformances, proto, type_id) {
                "is derived more than once"
            } else {
                "is already implemented manually"
            };
            self.error(
                codes::DUPLICATE_IMPL,
                format!("`{type_id}` conformance to `{proto}` {how}; a conformance must be unique"),
                span,
            );
            return;
        }
        catalog_insert_conformance(
            &mut self.conformances,
            proto.to_string(),
            type_id.to_string(),
        );
        catalog_insert_conformance(
            &mut self.derived_conformances,
            proto.to_string(),
            type_id.to_string(),
        );
    }

    /// Returns the first member that cannot provide one derive capability. Unlike
    /// the general expression predicates, this walker substitutes generic nominal
    /// members from their declaration before deciding, so `Box<(int) -> int>` can
    /// never pass an `Eq` derive merely because only `Box<T>` is stored in the
    /// nominal table. Recursive Eq/Order nominals are accepted (finite values);
    /// recursive JSON is rejected because the decoder has no recursive schema.
    pub(super) fn first_member_without_capability(
        &self,
        type_id: &str,
        is_enum: bool,
        capability: DeriveCapability,
    ) -> Option<String> {
        let mut seen = Vec::new();
        if is_enum {
            let info = self.enums.get(type_id)?;
            for v in &info.variants {
                for (i, ty) in v.payloads.iter().enumerate() {
                    if !self.derive_type_has_capability(ty, capability, &mut seen) {
                        return Some(format!("variant `{}` payload #{}", v.name, i));
                    }
                }
            }
            None
        } else {
            let info = self.records.get(type_id)?;
            for f in &info.fields {
                if !self.derive_type_has_capability(&f.ty, capability, &mut seen) {
                    return Some(format!("field `{}`", f.name));
                }
            }
            None
        }
    }

    pub(super) fn derive_type_has_capability(
        &self,
        ty: &Type,
        capability: DeriveCapability,
        seen: &mut Vec<String>,
    ) -> bool {
        match ty {
            Type::Enum { base, args } => {
                let key = nominal_instance_id(base, args);
                if seen.contains(&key) {
                    return capability != DeriveCapability::Json;
                }
                let Some(info) = self.enums.get(base) else {
                    return false;
                };
                if args.len() != info.type_params.len() {
                    return false;
                }
                seen.push(key);
                let ok = info.variants.iter().all(|variant| {
                    variant.payloads.iter().all(|payload| {
                        let concrete = substitute(payload, args);
                        self.derive_type_has_capability(&concrete, capability, seen)
                    })
                });
                seen.pop();
                ok
            }
            Type::NominalRecord { base, args } => {
                let key = nominal_instance_id(base, args);
                if seen.contains(&key) {
                    return capability != DeriveCapability::Json;
                }
                let Some(info) = self.records.get(base) else {
                    return false;
                };
                if args.len() != info.type_params.len() {
                    return false;
                }
                seen.push(key);
                let ok = info.fields.iter().all(|field| {
                    let concrete = substitute(&field.ty, args);
                    self.derive_type_has_capability(&concrete, capability, seen)
                });
                seen.pop();
                ok
            }
            Type::Newtype { base, args } => {
                let key = nominal_instance_id(base, args);
                if seen.contains(&key) {
                    return capability != DeriveCapability::Json;
                }
                let Some(info) = self.newtypes.get(base) else {
                    return false;
                };
                if args.len() != info.type_params.len() {
                    return false;
                }
                seen.push(key);
                let concrete = substitute(&info.base, args);
                let ok = self.derive_type_has_capability(&concrete, capability, seen);
                seen.pop();
                ok
            }
            Type::Prim(prim) => match capability {
                DeriveCapability::Eq => true,
                DeriveCapability::Order => matches!(prim, Prim::Int | Prim::String),
                DeriveCapability::Json => {
                    matches!(prim, Prim::Int | Prim::String | Prim::Bool | Prim::Unit)
                }
            },
            Type::Literal(lit) => match capability {
                DeriveCapability::Eq => true,
                DeriveCapability::Order => matches!(lit, Lit::Int(_) | Lit::Str(_)),
                DeriveCapability::Json => !matches!(lit, Lit::Float(_)),
            },
            Type::Union(members) => match capability {
                DeriveCapability::Eq => members
                    .iter()
                    .all(|member| self.derive_type_has_capability(member, capability, seen)),
                DeriveCapability::Order | DeriveCapability::Json => false,
            },
            Type::Record(fields) => match capability {
                DeriveCapability::Eq | DeriveCapability::Json => fields
                    .iter()
                    .all(|(_, field)| self.derive_type_has_capability(field, capability, seen)),
                DeriveCapability::Order => false,
            },
            Type::Ctor(ctor, args) => match capability {
                DeriveCapability::Eq => {
                    !matches!(ctor, Ctor::Map | Ctor::Set)
                        && args
                            .iter()
                            .all(|arg| self.derive_type_has_capability(arg, capability, seen))
                }
                DeriveCapability::Order => false,
                DeriveCapability::Json => match ctor {
                    Ctor::Option | Ctor::Array => args
                        .first()
                        .is_some_and(|arg| self.derive_type_has_capability(arg, capability, seen)),
                    Ctor::Map => {
                        matches!(
                            args.first(),
                            Some(Type::Prim(Prim::String) | Type::Literal(Lit::Str(_)))
                        ) && args.get(1).is_some_and(|value| {
                            self.derive_type_has_capability(value, capability, seen)
                        })
                    }
                    Ctor::Result | Ctor::Set | Ctor::Range => false,
                },
            },
            Type::Func { .. } => false,
            Type::Bytes | Type::Path | Type::Url | Type::Date | Type::BigInt | Type::Decimal => {
                matches!(capability, DeriveCapability::Eq | DeriveCapability::Order)
            }
            Type::Match => capability == DeriveCapability::Eq,
            Type::RoundingMode => capability == DeriveCapability::Eq,
            Type::JsonValue => capability == DeriveCapability::Json,
            Type::ByteBuffer
            | Type::Template
            | Type::File
            | Type::Regex
            | Type::TomlValue
            | Type::Foreign { .. }
            | Type::Skolem { .. }
            | Type::Unknown
            | Type::Var(_) => false,
        }
    }

    /// §4 (v5.4) the signature of a declared method `(type id, method)`, if any.
    pub(crate) fn method_info(&self, type_id: &str, method: &str) -> Option<&MethodInfo> {
        self.methods
            .get(type_id)?
            .get(method)
            .map(|info| &info.signature)
    }

    /// Stable runtime dispatch identity for a declared inherent receiver method.
    /// Local and imported methods use the same catalog entry as their signature,
    /// so call checking cannot accept a method while dropping its product target.
    pub(crate) fn receiver_method_dispatch_id(&self, type_id: &str, method: &str) -> Option<&str> {
        self.methods
            .get(type_id)?
            .get(method)?
            .dispatch_id
            .as_deref()
    }

    /// Connect every accepted own-module receiver method to its defining module.
    /// Import entries already carry their producer-owned identity.
    pub(crate) fn set_receiver_method_dispatch_module(&mut self, module: &str) {
        for nominal in &self.own_nominals {
            let Some(methods) = self.methods.get_mut(nominal) else {
                continue;
            };
            let dispatch_id = module_nominal_identity(module, nominal);
            for method in methods.values_mut() {
                method.dispatch_id = Some(dispatch_id.clone());
            }
        }
    }

    pub(crate) fn set_method_return(&mut self, type_id: &str, method: &str, ret: Type) {
        if let Some(info) = self
            .methods
            .get_mut(type_id)
            .and_then(|methods| methods.get_mut(method))
        {
            info.signature.ret = ret;
        }
    }

    /// The only inherent receiver declaration admitted here is a bare first
    /// `self`. The parser represents that special slot with a placeholder type at
    /// the exact same span as the parameter name; an explicitly annotated `self`
    /// necessarily has a distinct type span.
    pub(crate) fn is_bare_self_param(&self, param: &ast::Param) -> bool {
        self.text(param.name.span) == "self"
            && !param.variadic
            && param.default.is_none()
            && param.ty.span == param.name.span
    }
}
