use super::*;

/// Closed, source-spellable standard-library value types. Keeping this lookup
/// separate from receiver/member catalogs makes named-type formation own both
/// the semantic carrier and its zero-argument contract in one place.
pub(super) fn terminal_named_type(name: &str) -> Option<Type> {
    match name {
        "template" => Some(Type::Template),
        "JSONValue" => Some(Type::JsonValue),
        "File" => Some(Type::File),
        "Bytes" => Some(Type::Bytes),
        "ByteBuffer" => Some(Type::ByteBuffer),
        "Path" => Some(Type::Path),
        "Regex" => Some(Type::Regex),
        "Match" => Some(Type::Match),
        "TOMLValue" => Some(Type::TomlValue),
        "URL" => Some(Type::Url),
        "Date" => Some(Type::Date),
        "BigInt" => Some(Type::BigInt),
        "Decimal" => Some(Type::Decimal),
        "RoundingMode" => Some(Type::RoundingMode),
        _ => None,
    }
}

/// §3 (v5.3): describes the existing type-name a candidate enum name collides
/// with, or `None` if the name is free. Mirrors the resolution order of
/// `form_named`: a primitive, a builtin generic ctor, or an opaque library type.
pub(super) fn reserved_type_name_kind(name: &str) -> Option<&'static str> {
    match name {
        "int" | "float" | "string" | "bool" => Some("a primitive type"),
        "Array" | "Map" | "Set" | "Option" | "Result" => Some("a builtin type constructor"),
        "template" => Some("the builtin `template` type"),
        "JSONValue" => Some("the builtin `JSONValue` type"),
        "File" => Some("the builtin `File` type"),
        "Bytes" => Some("the builtin `Bytes` type"),
        "ByteBuffer" => Some("the builtin `ByteBuffer` type"),
        "Path" => Some("the builtin `Path` type"),
        "Regex" => Some("the builtin `Regex` type"),
        "Match" => Some("the builtin `Match` type"),
        "TOMLValue" => Some("the builtin `TOMLValue` type"),
        "URL" => Some("the builtin `URL` type"),
        "Date" => Some("the builtin `Date` type"),
        "BigInt" => Some("the builtin `BigInt` type"),
        "Decimal" => Some("the builtin `Decimal` type"),
        "RoundingMode" => Some("the builtin `RoundingMode` type"),
        _ => None,
    }
}

/// §3 (v5.3): describes what a candidate VARIANT name reserved-ly means, or
/// `None` if it is free. `None`/`Some`/`Ok`/`Err` are prelude constructors that
/// both engines treat as the prelude constructor, so a user variant with such a
/// name would diverge run≢build. (The literals `true`/`false`/`null` are not
/// Ident tokens, so they cannot reach here — the parser rejects them as variant
/// names with TPZ2001 before this gate.)
pub(super) fn reserved_variant_name_kind(name: &str) -> Option<&'static str> {
    match name {
        "None" | "Some" | "Ok" | "Err" => Some("a reserved prelude constructor"),
        _ => None,
    }
}

/// Replaces `Var(i)` placeholders with the given arguments (an
/// exported alias body instantiation).
pub(super) fn substitute_params(body: &Type, args: &[Type]) -> Type {
    body.transform_components(&mut |component| match component {
        Type::Var(index) => Some(args.get(*index as usize).cloned().unwrap_or(Type::Unknown)),
        _ => None,
    })
}

/// Applies a short-circuiting predicate to a source type and its structural
/// children in source order. Semantic heads remain the caller's responsibility;
/// this function owns only the complete `ast::TypeKind` child inventory.
pub(super) fn type_syntax_any(
    ty: &ast::Type,
    predicate: &mut impl FnMut(&ast::Type) -> bool,
) -> bool {
    if predicate(ty) {
        return true;
    }
    match &ty.kind {
        ast::TypeKind::Unit | ast::TypeKind::Literal => false,
        ast::TypeKind::Named { args, .. } | ast::TypeKind::Qualified { args, .. } => {
            args.iter().any(|arg| type_syntax_any(arg, predicate))
        }
        ast::TypeKind::Union(members) => members
            .iter()
            .any(|member| type_syntax_any(member, predicate)),
        ast::TypeKind::Record(fields) => fields
            .iter()
            .any(|field| type_syntax_any(&field.ty, predicate)),
        ast::TypeKind::Function { params, ret } => {
            params
                .iter()
                .any(|param| type_syntax_any(&param.ty, predicate))
                || type_syntax_any(ret, predicate)
        }
    }
}

pub(crate) fn substitute(ty: &Type, args: &[Type]) -> Type {
    ty.transform_components(&mut |component| match component {
        Type::Var(index) => Some(
            args.get(*index as usize)
                .cloned()
                .unwrap_or(Type::Var(*index)),
        ),
        _ => None,
    })
}

impl<'a> Former<'a> {
    /// Collects the program's alias table at the CURRENT language version (the
    /// convenience entry); use [`Self::with_version`] to pin the edition.
    /// Duplicate alias names are redeclarations (TPZ5008); duplicate type
    /// parameters are malformed (TPZ5022).
    pub fn new(src: &'a str, program: &'a ast::Program) -> Self {
        Self::with_version(src, program, LangVersion::CURRENT)
    }

    /// [`Self::new`] pinned to a language `version` (the checker threads the
    /// `--language-version` selection so v5.4-only enum features gate by edition).
    pub fn with_version(src: &'a str, program: &'a ast::Program, version: LangVersion) -> Self {
        let mut former = Self::seed(src, version);
        former.collect_frame(&program.items);
        former.collect_nominals(&program.items);
        former.collect_protocols(&program.items);
        former.collect_methods(&program.items);
        former.collect_derives(&program.items);
        former
    }

    /// Constructs a module checker in dependency-aware order: local names are
    /// registered first, imports become available next, and only then are local
    /// protocol signatures and nominal members formed. This prevents imported
    /// signature/member types from being frozen as same-spelled foreign
    /// placeholders.
    pub(crate) fn with_module_context(
        src: &'a str,
        program: &'a ast::Program,
        version: LangVersion,
        ctx: ModuleContext,
    ) -> Self {
        let mut former = Self::seed(src, version);
        former.collect_frame(&program.items);
        let pending = former.collect_nominal_names(&program.items);
        former.set_module_context(ctx);
        former.collect_protocols(&program.items);
        former.form_nominal_members(pending);
        former.collect_methods(&program.items);
        former.collect_derives(&program.items);
        former
    }

    pub(super) fn seed(src: &'a str, version: LangVersion) -> Self {
        Former {
            src,
            report_type_diagnostics: true,
            formed_signature_types: HashSet::new(),
            aliases: vec![HashMap::new()],
            expanding: Vec::new(),
            validation_base: HashMap::new(),
            module_mode: false,
            namespace_aliases: HashMap::new(),
            namespace_records: HashMap::new(),
            namespace_enums: HashMap::new(),
            namespace_newtypes: HashMap::new(),
            ambient_namespaces: HashSet::new(),
            imported_aliases: HashMap::new(),
            imported_enum_sources: HashMap::new(),
            imported_newtype_sources: HashMap::new(),
            imported_schema_nominals: HashSet::new(),
            enums: HashMap::new(),
            own_nominals: HashSet::new(),
            records: HashMap::new(),
            newtypes: HashMap::new(),
            methods: HashMap::new(),
            accepted_receiver_methods: HashSet::new(),
            protocols: HashMap::new(),
            protocol_methods: HashMap::new(),
            conformances: HashMap::new(),
            derived_conformances: HashMap::new(),
            version,
            diagnostics: Vec::new(),
        }
    }

    /// Read-only access to the §4 derive conformance table (set of
    /// `(protocol, type_id)` pairs). Consulted by protocol-call dispatch and by
    /// exported typed observations.
    pub(crate) fn conformances(&self) -> impl Iterator<Item = (&str, &str)> {
        self.conformances.iter().flat_map(|(protocol, types)| {
            types
                .iter()
                .map(move |type_id| (protocol.as_str(), type_id.as_str()))
        })
    }

    /// Module boundary: only derived conformances to the four globally
    /// predeclared protocols can cross a module interface. User protocols and
    /// manual impl bodies have no exported definition/body surface, so exporting
    /// their name-only conformance key would let an importer accidentally match a
    /// different same-spelled protocol.
    pub(crate) fn exportable_conformances(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (protocol, types) in &self.derived_conformances {
            if !matches!(protocol.as_str(), "Eq" | "Order" | "Show" | "JSON") {
                continue;
            }
            out.extend(
                types
                    .iter()
                    .map(|type_id| (protocol.clone(), type_id.clone())),
            );
        }
        out.sort();
        out
    }

    /// §4 (v5.4) whether `(protocol, type_id)` conforms — by `derives` or a manual
    /// `impl`. Consulted by a `Protocol.method(x)` dispatch (the receiver's type must
    /// conform) before the call is admitted.
    pub(crate) fn conforms(&self, protocol: &str, type_id: &str) -> bool {
        catalog_contains_conformance(&self.conformances, protocol, type_id)
    }

    /// §4 (v5.4) a declared protocol's surface, if any (incl. the predeclared
    /// builtins `Show`/`Eq`/`Order`).
    pub(crate) fn protocol(&self, name: &str) -> Option<&ProtocolInfo> {
        self.protocols.get(name)
    }

    /// §4 (v5.4) a MANUAL protocol-impl method's signature `(protocol, type_id,
    /// method)`, if registered (a `Protocol.method(x)` call on a manually-conforming
    /// type checks its args against this CONCRETE signature).
    pub(crate) fn protocol_method(
        &self,
        protocol: &str,
        type_id: &str,
        method: &str,
    ) -> Option<&MethodInfo> {
        self.protocol_methods
            .get(protocol)?
            .get(type_id)?
            .get(method)
    }

    /// Whether this exact receiver-method declaration survived formation and
    /// therefore owns a product body.
    pub(crate) fn receiver_method_was_accepted(&self, declaration: Span) -> bool {
        self.accepted_receiver_methods.contains(&declaration)
    }

    /// The language version of this check session.
    pub(crate) fn version(&self) -> LangVersion {
        self.version
    }

    /// Source names of every nominal declaration owned by this module. Import
    /// aliases and canonical identities installed from dependencies are not
    /// members of this set.
    pub(crate) fn own_nominal_names(&self) -> impl Iterator<Item = &str> {
        self.own_nominals.iter().map(String::as_str)
    }

    /// Pre-collects one lexical level's aliases (block-level
    /// hoisting: forward references resolve within the frame).
    pub(crate) fn collect_frame(&mut self, items: &'a [ast::Stmt]) {
        for stmt in items {
            let alias = match &stmt.kind {
                ast::StmtKind::TypeAlias(a) => a,
                ast::StmtKind::Export(inner) => {
                    if let ast::StmtKind::TypeAlias(a) = &inner.kind {
                        a
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };
            self.collect_alias(alias);
        }
    }

    pub(crate) fn push_alias_frame(&mut self) {
        self.aliases.push(HashMap::new());
    }

    pub(crate) fn pop_alias_frame(&mut self) {
        self.aliases.pop();
    }

    pub(super) fn alias_lookup(&self, name: &str) -> bool {
        self.aliases.iter().rev().any(|f| f.contains_key(name))
    }

    pub(super) fn collect_alias(&mut self, alias: &'a ast::TypeAlias) {
        let name = self.text(alias.name.span);
        let mut params: Vec<&'a str> = Vec::new();
        for p in &alias.type_params {
            let pname = self.text(p.span);
            if params.contains(&pname) {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!("type parameter `{pname}` is declared twice"),
                    p.span,
                );
            } else {
                params.push(pname);
            }
        }
        let frame = self.aliases.last().expect("alias frame");
        if let Some(prev) = frame.get(name) {
            let prev_span = prev.name_span;
            self.diagnostics.push(
                Diagnostic::error(
                    codes::REDECLARE,
                    format!("type alias `{name}` is declared twice"),
                    Label::new(alias.name.span, "redeclared here"),
                )
                .with_secondary(prev_span, "first declaration"),
            );
            return;
        }
        self.aliases.last_mut().expect("alias frame").insert(
            name,
            AliasDef {
                params,
                body: &alias.ty,
                name_span: alias.name.span,
                resolved: None,
            },
        );
    }

    pub fn source(&self) -> &'a str {
        self.src
    }

    pub(crate) fn text(&self, span: Span) -> &'a str {
        &self.src[span.lo as usize..span.hi as usize]
    }

    pub(crate) fn error(&mut self, code: Code, message: String, span: Span) {
        if self.report_type_diagnostics {
            self.diagnostics
                .push(Diagnostic::error(code, message, Label::new(span, "")));
        }
    }

    /// Validates every alias body in the innermost frame once, with
    /// its parameters in scope.
    pub fn validate_aliases(&mut self) {
        self.validate_aliases_in(&HashMap::new());
    }

    /// Like [`Self::validate_aliases`], layering the alias
    /// parameters over a base environment — block-local aliases
    /// inside a generic function must see the function's rigid type
    /// parameters (SPEC §5/§7).
    pub fn validate_aliases_in(&mut self, base: &HashMap<&'a str, Type>) {
        self.validation_base = base.clone();
        let frame = self.aliases.last().expect("alias frame");
        let mut names: Vec<&'a str> = frame.keys().copied().collect();
        names.sort_unstable();
        for name in names {
            let (params, body) = {
                let def = self
                    .aliases
                    .last()
                    .expect("alias frame")
                    .get(name)
                    .expect("validated name");
                (def.params.clone(), def.body)
            };
            let mut env: HashMap<&'a str, Type> = base.clone();
            env.extend(
                params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (*p, Type::Var(i as u32))),
            );
            self.expanding.push(name);
            let resolved = self.form(body, &env);
            self.expanding.pop();
            if let Some(def) = self.aliases.last_mut().expect("alias frame").get_mut(name) {
                def.resolved = Some(resolved);
            }
        }
        self.validation_base.clear();
    }

    /// Forms a checker type from an AST type expression. `env` maps
    /// in-scope type parameters to their representations.
    pub fn form(&mut self, ty: &'a ast::Type, env: &HashMap<&'a str, Type>) -> Type {
        match &ty.kind {
            ast::TypeKind::Unit => Type::Prim(Prim::Unit),
            ast::TypeKind::Literal => self.form_literal(ty.span),
            ast::TypeKind::Union(members) => {
                let formed = members.iter().map(|m| self.form(m, env)).collect();
                Type::union(formed)
            }
            ast::TypeKind::Record(fields) => self.form_record(fields, env),
            ast::TypeKind::Function { params, ret } => {
                self.form_function(params, ret, env, ty.span)
            }
            ast::TypeKind::Named { name, args } => self.form_named(name, args, env),
            ast::TypeKind::Qualified { ns, name, args } => {
                let ns_text = self.text(ns.span);
                let n = self.text(name.span);
                let formed: Vec<Type> = args.iter().map(|a| self.form(a, env)).collect();
                // Fragment mode and ambient namespaces (cycles) stay
                // opaque and identity-compared.
                if !self.module_mode || self.ambient_namespaces.contains(ns_text) {
                    return Type::Foreign {
                        name: format!("{ns_text}.{n}"),
                        args: formed,
                    };
                }
                match self
                    .namespace_aliases
                    .get(ns_text)
                    .map(|table| table.get(n).cloned())
                {
                    Some(Some(alias)) => {
                        let display = format!("{ns_text}.{n}");
                        let instantiated =
                            self.apply_exported_alias(&display, &alias, formed, ty.span);
                        self.qualify_namespace_type(ns_text, &instantiated)
                    }
                    Some(None) => {
                        if let Some(record) = self
                            .namespace_records
                            .get(ns_text)
                            .and_then(|table| table.get(n))
                            .cloned()
                        {
                            let display = format!("{ns_text}.{n}");
                            let template = record_info_from_export(record);
                            let base = self
                                .record_base_for_name(&display)
                                .unwrap_or_else(|| template.id.clone());
                            if self
                                .instantiate_record_template(
                                    &display,
                                    &base,
                                    template,
                                    formed.clone(),
                                    ty.span,
                                )
                                .is_some()
                            {
                                Type::NominalRecord { base, args: formed }
                            } else {
                                Type::Unknown
                            }
                        } else if let Some(enm) = self
                            .namespace_enums
                            .get(ns_text)
                            .and_then(|table| table.get(n))
                            .cloned()
                        {
                            let display = format!("{ns_text}.{n}");
                            let template = enum_info_from_export(enm);
                            let base = self
                                .enum_base_for_name(&display)
                                .unwrap_or_else(|| template.id.clone());
                            if self
                                .instantiate_enum_template(
                                    &display,
                                    &base,
                                    template,
                                    formed.clone(),
                                    ty.span,
                                )
                                .is_some()
                            {
                                Type::Enum { base, args: formed }
                            } else {
                                Type::Unknown
                            }
                        } else if let Some(newtype) = self
                            .namespace_newtypes
                            .get(ns_text)
                            .and_then(|table| table.get(n))
                            .cloned()
                        {
                            let display = format!("{ns_text}.{n}");
                            let template = newtype_info_from_export(newtype);
                            let base = self
                                .newtype_base_for_name(&display)
                                .unwrap_or_else(|| template.id.clone());
                            if self
                                .instantiate_newtype_template(
                                    &display,
                                    &base,
                                    template,
                                    formed.clone(),
                                    ty.span,
                                )
                                .is_some()
                            {
                                Type::Newtype { base, args: formed }
                            } else {
                                Type::Unknown
                            }
                        } else {
                            self.error(
                                codes::INVALID_QUALIFIED,
                                format!("`{n}` is not an exported type of `{ns_text}` (§17)"),
                                ty.span,
                            );
                            Type::Unknown
                        }
                    }
                    None => {
                        self.error(
                            codes::INVALID_QUALIFIED,
                            format!("`{ns_text}` is not an imported namespace (§17)"),
                            ty.span,
                        );
                        Type::Unknown
                    }
                }
            }
        }
    }

    /// Forms an annotation in the declaration/signature phase and records that
    /// this exact source occurrence owns its well-formedness diagnostics.
    pub(crate) fn form_signature(
        &mut self,
        ty: &'a ast::Type,
        env: &HashMap<&'a str, Type>,
    ) -> Type {
        self.formed_signature_types.insert(ty.span);
        self.form(ty, env)
    }

    /// Re-forms an annotation for a body. When its exact source occurrence was
    /// already formed by a declaration/signature pass, the result is still needed
    /// to replace scheme variables with rigid body variables but repeating the
    /// same diagnostic is not a second product observation. Structurally rejected
    /// protocol methods that skipped signature formation remain diagnostic owners.
    pub(crate) fn form_for_body(
        &mut self,
        ty: &'a ast::Type,
        env: &HashMap<&'a str, Type>,
    ) -> Type {
        if !self.formed_signature_types.contains(&ty.span) {
            return self.form(ty, env);
        }
        let previous = self.report_type_diagnostics;
        self.report_type_diagnostics = false;
        let formed = self.form(ty, env);
        self.report_type_diagnostics = previous;
        formed
    }

    pub(super) fn form_literal(&mut self, span: Span) -> Type {
        let text = self.text(span);
        // Raw span text: escape/multiline cooking arrives with the
        // expression-literal machinery; literal-type identity stays
        // textual until then.
        let lit = if let Some(inner) = text.strip_prefix('"') {
            Lit::Str(inner.strip_suffix('"').unwrap_or(inner).to_string())
        } else if text == "true" {
            Lit::Bool(true)
        } else if text == "false" {
            Lit::Bool(false)
        } else if text == "null" {
            Lit::Null
        } else if text.contains('.') || text.contains('e') || text.contains('E') {
            Lit::Float(text.to_string())
        } else {
            match text.replace('_', "").parse::<i64>() {
                Ok(n) => Lit::Int(n),
                Err(_) => {
                    self.error(
                        codes::MALFORMED_TYPE,
                        format!("integer literal type `{text}` is out of range"),
                        span,
                    );
                    return Type::Var(u32::MAX);
                }
            }
        };
        Type::Literal(lit)
    }

    pub(super) fn form_record(
        &mut self,
        fields: &'a [ast::FieldType],
        env: &HashMap<&'a str, Type>,
    ) -> Type {
        let mut formed: Vec<(String, Type)> = Vec::new();
        for field in fields {
            let name = self.text(field.name.span).to_string();
            if formed.iter().any(|(n, _)| *n == name) {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!("record type declares field `{name}` twice"),
                    field.span,
                );
                continue;
            }
            let ty = self.form(&field.ty, env);
            formed.push((name, ty));
        }
        formed.sort_by(|(a, _), (b, _)| a.cmp(b));
        Type::Record(formed)
    }

    pub(super) fn form_function(
        &mut self,
        params: &'a [ast::FunctionTypeParam],
        ret: &'a ast::Type,
        env: &HashMap<&'a str, Type>,
        span: Span,
    ) -> Type {
        let mut formed = Vec::new();
        let mut variadic: Option<Box<Type>> = None;
        for (i, p) in params.iter().enumerate() {
            let t = self.form(&p.ty, env);
            if p.variadic {
                if i + 1 != params.len() {
                    // SPEC §3: a variadic function-type parameter
                    // must be final.
                    self.error(
                        codes::VARIADIC_POSITION,
                        "a variadic function-type parameter must be final".to_string(),
                        span,
                    );
                }
                variadic = Some(Box::new(t));
            } else {
                formed.push(t);
            }
        }
        Type::Func {
            params: formed,
            variadic,
            ret: Box::new(self.form(ret, env)),
        }
    }

    pub(super) fn form_named(
        &mut self,
        name: &ast::Ident,
        args: &'a [Rc<ast::Type>],
        env: &HashMap<&'a str, Type>,
    ) -> Type {
        let text = self.text(name.span);

        if let Some(param) = env.get(text) {
            if !args.is_empty() {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!("type parameter `{text}` takes no type arguments"),
                    name.span,
                );
            }
            return param.clone();
        }

        let prim = match text {
            "int" => Some(Prim::Int),
            "float" => Some(Prim::Float),
            "string" => Some(Prim::String),
            "bool" => Some(Prim::Bool),
            _ => None,
        };
        if let Some(p) = prim {
            if !args.is_empty() {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!("primitive type `{text}` takes no type arguments"),
                    name.span,
                );
            }
            return Type::Prim(p);
        }

        if let Some((ctor, arity)) = Ctor::from_name(text) {
            if args.len() != arity {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!(
                        "`{text}` takes {arity} type argument{}, found {}",
                        if arity == 1 { "" } else { "s" },
                        args.len()
                    ),
                    name.span,
                );
                return Type::Var(u32::MAX);
            }
            let formed: Vec<Type> = args.iter().map(|a| self.form(a, env)).collect();
            // §3/§6 (v5.4) the key/element slot of `Map`/`Set` annotations must
            // satisfy the same runtime-freeze keyability rule as literal and
            // `Set.of`/`Map.new` inference. (`Map`'s VALUE slot and any other
            // position are unaffected — only the KEY/element is a key.)
            if matches!(ctor, Ctor::Map | Ctor::Set)
                && let Some(bad) = non_keyable_map_set_key_with_nominals(
                    &formed[0],
                    |id| self.newtype_info(id).map(|info| info.base.clone()),
                    |id| {
                        self.record_info(id)
                            .map(|info| info.fields.iter().map(|field| field.ty.clone()).collect())
                    },
                    |id| {
                        self.enum_info(id).map(|info| {
                            info.variants
                                .iter()
                                .flat_map(|variant| variant.payloads.iter().cloned())
                                .collect()
                        })
                    },
                )
            {
                self.error(
                    codes::INCOMPARABLE,
                    format!(
                        "{} is not a valid Map/Set key ({} keys are not supported yet)",
                        bad.subject, bad.kind
                    ),
                    name.span,
                );
            }
            return Type::Ctor(ctor, formed);
        }

        // Closed standard-library value types are zero-argument named types. The
        // same catalog also includes `File`, so a source annotation and the value
        // returned by `open` carry one semantic type instead of two identically
        // rendered identities.
        if let Some(terminal) = terminal_named_type(text) {
            if !args.is_empty() {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!("`{text}` takes no type arguments"),
                    name.span,
                );
            }
            return terminal;
        }

        // §3 a declared user enum (v5.3/v5.4) resolves to its nominal type.
        // Generic enum declarations instantiate at concrete type use sites.
        if self.is_enum(text) {
            let formed: Vec<Type> = args.iter().map(|a| self.form(a, env)).collect();
            let base = self
                .enum_base_for_name(text)
                .unwrap_or_else(|| text.to_string());
            return if self
                .enum_instance(text, formed.clone(), name.span)
                .is_some()
            {
                Type::Enum { base, args: formed }
            } else {
                Type::Unknown
            };
        }

        // §3 a declared user nominal record (v5.4) resolves to its nominal type.
        // Generic record declarations instantiate at concrete type use sites.
        if self.is_record(text) {
            let formed: Vec<Type> = args.iter().map(|a| self.form(a, env)).collect();
            let base = self
                .record_base_for_name(text)
                .unwrap_or_else(|| text.to_string());
            return if self
                .record_instance(text, formed.clone(), name.span)
                .is_some()
            {
                Type::NominalRecord { base, args: formed }
            } else {
                Type::Unknown
            };
        }

        // §3 a declared user newtype (v5.4) resolves to its nominal type.
        // Generic newtypes instantiate at concrete type use sites.
        if self.is_newtype(text) {
            let formed: Vec<Type> = args.iter().map(|a| self.form(a, env)).collect();
            let base = self
                .newtype_base_for_name(text)
                .unwrap_or_else(|| text.to_string());
            return if self
                .newtype_instance(text, formed.clone(), name.span)
                .is_some()
            {
                Type::Newtype { base, args: formed }
            } else {
                Type::Unknown
            };
        }

        if self.alias_lookup(text) {
            return self.expand_alias(text, name.span, args, env);
        }

        if let Some(alias) = self.imported_aliases.get(text).cloned() {
            let formed: Vec<Type> = args.iter().map(|a| self.form(a, env)).collect();
            return self.apply_exported_alias(text, &alias, formed, name.span);
        }

        // An undeclared type name is not diagnosable in a staged
        // single-program pass: documentation snippets (and modules
        // before module-aware checking) legitimately reference types
        // declared elsewhere — the same ambient-name posture the
        // interpreter takes with TPZ5002. The name forms as an
        // opaque, identity-compared type; its arguments still form.
        let formed = args.iter().map(|a| self.form(a, env)).collect();
        Type::Foreign {
            name: text.to_string(),
            args: formed,
        }
    }

    pub(super) fn expand_alias(
        &mut self,
        name: &'a str,
        use_span: Span,
        args: &'a [Rc<ast::Type>],
        env: &HashMap<&'a str, Type>,
    ) -> Type {
        if self.expanding.contains(&name) {
            // SPEC §3: recursive alias cycles are static errors.
            self.error(
                codes::ALIAS_CYCLE,
                format!("type alias `{name}` refers to itself (alias cycle)"),
                use_span,
            );
            return Type::Var(u32::MAX);
        }
        let def = self
            .aliases
            .iter()
            .rev()
            .find_map(|f| f.get(name))
            .expect("alias_lookup guarded");
        let (params, body, resolved) = (def.params.clone(), def.body, def.resolved.clone());
        if args.len() != params.len() {
            self.error(
                codes::MALFORMED_TYPE,
                format!(
                    "type alias `{name}` takes {} type argument{}, found {}",
                    params.len(),
                    if params.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                use_span,
            );
            return Type::Var(u32::MAX);
        }
        let formed_args: Vec<Type> = args.iter().map(|a| self.form(a, env)).collect();
        match resolved {
            // The cached definition-site body: use-site frames can
            // never re-bind the names it mentions.
            Some(cached) => substitute(&cached, &formed_args),
            // Within the defining frame's own validation pass the
            // cache is not built yet; forming from the body is safe
            // there because the frame IS the definition environment.
            None => {
                let mut inner_env: HashMap<&'a str, Type> = self.validation_base.clone();
                inner_env.extend(params.into_iter().zip(formed_args));
                self.expanding.push(name);
                let formed = self.form(body, &inner_env);
                self.expanding.pop();
                formed
            }
        }
    }
}
