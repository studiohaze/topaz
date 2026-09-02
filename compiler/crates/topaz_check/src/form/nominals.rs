use super::*;

pub(super) fn nominal_type_env<'a>(params: &[&'a str]) -> HashMap<&'a str, Type> {
    params
        .iter()
        .enumerate()
        .map(|(i, p)| (*p, Type::Var(i as u32)))
        .collect()
}

pub(super) fn synthetic_type_params(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("T{i}")).collect()
}

pub(crate) fn nominal_instance_id(name: &str, args: &[Type]) -> String {
    if args.is_empty() {
        return name.to_string();
    }
    let args = args
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name}<{args}>")
}

pub(super) fn has_invalid_nominal_instance_arg(ty: &Type) -> bool {
    ty.any_component(&mut |component| matches!(component, Type::Unknown | Type::Var(u32::MAX)))
}

pub(super) fn nominal_instance_arg_has_scope_identity(ty: &Type) -> bool {
    ty.any_component(&mut |component| matches!(component, Type::Var(_) | Type::Skolem { .. }))
}

pub(super) fn nominal_instance_args_have_scope_identity(args: &[Type]) -> bool {
    args.iter().any(nominal_instance_arg_has_scope_identity)
}

impl<'a> Former<'a> {
    /// Pre-collects every top-level user-enum declaration (top level only), with
    /// the full §3 declaration-hygiene gates, in TWO PHASES (v5.4) so a payload
    /// type may refer to ANY enum declared in the module — including the enum's
    /// own type (`enum Expr { Bin(Expr, Expr) }`) and a mutually-referential enum
    /// (`enum A { X(B) } enum B { Y(A) }`):
    ///
    ///   Phase 1 — register every accepted enum NAME with an empty placeholder
    ///     `EnumInfo` (so `form_named` resolves it to `Type::Enum`), and record
    ///     the accepted variants (name + AST payload types) for phase 2.
    ///   Phase 2 — FORM each accepted variant's payload types. Now every enum name
    ///     is known, so a recursive/mutual payload forms as `Type::Enum`, not
    ///     `Foreign`. `Type::Enum` is TERMINAL in the display/substitute/
    ///     has_unknown walkers, so no infinite type expansion.
    ///
    /// Runs AFTER `collect_frame` so the alias table is populated for the
    /// name-collision check. A duplicate enum name is a redeclaration (TPZ5008); a
    /// name colliding with a type alias / builtin ctor / primitive / opaque library
    /// type is malformed (TPZ5022). Per VARIANT: a duplicate variant is a
    /// redeclaration; a reserved prelude-constructor name (`None`/`Some`/`Ok`/`Err`)
    /// is rejected and SKIPPED so the prelude meaning is preserved everywhere.
    pub(super) fn collect_nominals(&mut self, items: &'a [ast::Stmt]) {
        let pending = self.collect_nominal_names(items);
        self.form_nominal_members(pending);
    }

    /// Phase 1 only: register this module's nominal names and retain the member
    /// ASTs. Module-aware construction installs imports between this pass and
    /// [`Self::form_nominal_members`].
    pub(super) fn collect_nominal_names(&mut self, items: &'a [ast::Stmt]) -> PendingNominals<'a> {
        // ── Phase 1: register EVERY nominal NAME (enums AND records) with an
        // empty placeholder, after hygiene + collision checks (against aliases,
        // builtins, AND each other). This UNIFIED pass is required for record↔enum
        // mutual recursion + forward references: phase 1 must know all nominal
        // names before phase 2 forms ANY enum payload or record field, so e.g.
        // `record Wrap { e: Color } enum Color { … }` and the reverse both form
        // nominally (not as `Foreign`). The pending vecs carry the accepted
        // members to form in phase 2.
        let mut pending_enums: Vec<PendingEnum<'a>> = Vec::new();
        let mut pending_records: Vec<PendingRecord<'a>> = Vec::new();
        let mut pending_newtypes: Vec<PendingNewtype<'a>> = Vec::new();
        for stmt in items {
            let inner = match &stmt.kind {
                ast::StmtKind::Export(inner) => &**inner,
                _ => stmt,
            };
            match &inner.kind {
                ast::StmtKind::Enum(decl) => self.collect_enum_phase1(decl, &mut pending_enums),
                ast::StmtKind::Record(decl) => {
                    self.collect_record_phase1(decl, &mut pending_records)
                }
                ast::StmtKind::Newtype(decl) => {
                    self.collect_newtype_phase1(decl, &mut pending_newtypes)
                }
                _ => {}
            }
        }

        (pending_enums, pending_records, pending_newtypes)
    }

    /// Phase 2 only: form local nominal members after every local nominal name
    /// and, in module mode, every imported type has been registered.
    pub(super) fn form_nominal_members(&mut self, pending: PendingNominals<'a>) {
        let (pending_enums, pending_records, pending_newtypes) = pending;
        // ── Phase 2: form every accepted enum payload AND record field type. All
        // nominal names are now registered, so a recursive/mutual reference forms
        // as `Type::Enum`/`Type::NominalRecord` (TERMINAL in the walkers → no
        // infinite expansion). Generic declarations form their members against
        // Var(i) placeholders; concrete use sites substitute those placeholders.
        for (name, params, accepted) in pending_enums {
            let env = nominal_type_env(&params);
            let formed: Vec<EnumVariantInfo> = accepted
                .into_iter()
                .map(|(vname, tys)| EnumVariantInfo {
                    name: vname,
                    payloads: tys.iter().map(|t| self.form(t, &env)).collect(),
                })
                .collect();
            self.enums
                .get_mut(&name)
                .expect("registered in phase 1")
                .variants = formed;
        }
        for (name, params, accepted) in pending_records {
            let env = nominal_type_env(&params);
            let formed: Vec<RecordFieldInfo> = accepted
                .into_iter()
                .map(|(fname, ty, has_default)| RecordFieldInfo {
                    name: fname,
                    ty: self.form(ty, &env),
                    has_default,
                })
                .collect();
            self.records
                .get_mut(&name)
                .expect("registered in phase 1")
                .fields = formed;
        }
        for (name, params, base_ast) in pending_newtypes {
            let env = nominal_type_env(&params);
            let base = self.form(base_ast, &env);
            self.newtypes
                .get_mut(&name)
                .expect("registered in phase 1")
                .base = base;
        }
    }

    /// Whether an own-module nominal head declares one or more invariant type
    /// parameters. Inherent receiver impls deliberately exclude these until the
    /// language defines an impl binder and receiver substitution model.
    pub(crate) fn nominal_is_generic(&self, name: &str) -> bool {
        self.records
            .get(name)
            .is_some_and(|info| !info.type_params.is_empty())
            || self
                .enums
                .get(name)
                .is_some_and(|info| !info.type_params.is_empty())
            || self
                .newtypes
                .get(name)
                .is_some_and(|info| !info.type_params.is_empty())
    }

    /// Phase 1 for ONE enum: hygiene + register the name with an empty
    /// `EnumInfo`, push its accepted variants for phase-2 formation.
    pub(super) fn collect_enum_phase1(
        &mut self,
        decl: &'a ast::EnumDecl,
        pending: &mut Vec<PendingEnum<'a>>,
    ) {
        let name = self.text(decl.name.span).to_string();
        if self.nominal_name_taken(&name, decl.name.span, "enum") {
            return;
        }
        let params = self.collect_nominal_type_params(&decl.type_params);
        let mut accepted: Vec<PendingVariant<'a>> = Vec::new();
        let mut placeholder: Vec<EnumVariantInfo> = Vec::new();
        for v in &decl.variants {
            let vname = self.text(v.name.span).to_string();
            if let Some(kind) = reserved_variant_name_kind(&vname) {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!(
                        "enum variant `{vname}` collides with {kind}; choose another variant name"
                    ),
                    v.span,
                );
                // Skip it: the reserved name keeps its prelude/literal meaning.
                continue;
            }
            if accepted.iter().any(|(prev, _)| *prev == vname) {
                self.error(
                    codes::REDECLARE,
                    format!("enum variant `{vname}` is declared twice in `{name}`"),
                    v.span,
                );
                continue;
            }
            let payload_tys: &'a [ast::Type] = v.payload.as_deref().unwrap_or(&[]);
            accepted.push((vname.clone(), payload_tys));
            placeholder.push(EnumVariantInfo {
                name: vname,
                payloads: Vec::new(),
            });
        }
        self.enums.insert(
            name.clone(),
            EnumInfo {
                id: name.clone(),
                type_params: params.iter().map(|p| (*p).to_string()).collect(),
                variants: placeholder,
            },
        );
        self.own_nominals.insert(name.clone());
        pending.push((name, params, accepted));
    }

    /// Phase 1 for ONE record: hygiene + register the name with an empty
    /// `RecordInfo`, push its accepted fields for phase-2 formation.
    pub(super) fn collect_record_phase1(
        &mut self,
        decl: &'a ast::RecordDecl,
        pending: &mut Vec<PendingRecord<'a>>,
    ) {
        let name = self.text(decl.name.span).to_string();
        if self.nominal_name_taken(&name, decl.name.span, "record") {
            return;
        }
        let params = self.collect_nominal_type_params(&decl.type_params);
        let mut accepted: Vec<PendingField<'a>> = Vec::new();
        let mut placeholder: Vec<RecordFieldInfo> = Vec::new();
        for f in &decl.fields {
            let fname = self.text(f.name.span).to_string();
            if accepted.iter().any(|(prev, _, _)| *prev == fname) {
                self.error(
                    codes::REDECLARE,
                    format!("record field `{fname}` is declared twice in `{name}`"),
                    f.span,
                );
                continue;
            }
            accepted.push((fname.clone(), &f.ty, f.default.is_some()));
            placeholder.push(RecordFieldInfo {
                name: fname,
                ty: Type::Unknown,
                has_default: f.default.is_some(),
            });
        }
        self.records.insert(
            name.clone(),
            RecordInfo {
                id: name.clone(),
                type_params: params.iter().map(|p| (*p).to_string()).collect(),
                fields: placeholder,
            },
        );
        self.own_nominals.insert(name.clone());
        pending.push((name, params, accepted));
    }

    /// Phase 1 for ONE newtype: hygiene + register the name with a placeholder
    /// `NewtypeInfo` (base `Unknown`), push its AST base type for phase-2 formation.
    pub(super) fn collect_newtype_phase1(
        &mut self,
        decl: &'a ast::NewtypeDecl,
        pending: &mut Vec<PendingNewtype<'a>>,
    ) {
        let name = self.text(decl.name.span).to_string();
        if self.nominal_name_taken(&name, decl.name.span, "newtype") {
            return;
        }
        let params = self.collect_nominal_type_params(&decl.type_params);
        self.newtypes.insert(
            name.clone(),
            NewtypeInfo {
                id: name.clone(),
                type_params: params.iter().map(|p| (*p).to_string()).collect(),
                base: Type::Unknown,
            },
        );
        self.own_nominals.insert(name.clone());
        pending.push((name, params, &decl.base));
    }

    /// Shared phase-1 nominal-name hygiene: reject (and report) a name that
    /// collides with a builtin/primitive/library type, a type alias, OR another
    /// already-registered nominal (enum/record). Returns `true` when the name is
    /// taken (so the caller skips registering it).
    pub(super) fn collect_nominal_type_params(&mut self, params: &[ast::Ident]) -> Vec<&'a str> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for p in params {
            let name = self.text(p.span);
            if !seen.insert(name) {
                self.error(
                    codes::MALFORMED_TYPE,
                    format!("type parameter `{name}` is declared twice"),
                    p.span,
                );
                continue;
            }
            out.push(name);
        }
        out
    }

    pub(super) fn nominal_name_taken(&mut self, name: &str, span: Span, kind: &str) -> bool {
        if self.enums.contains_key(name)
            || self.records.contains_key(name)
            || self.newtypes.contains_key(name)
        {
            self.error(
                codes::REDECLARE,
                format!("nominal type `{name}` is declared twice"),
                span,
            );
            return true;
        }
        if let Some(what) = reserved_type_name_kind(name) {
            self.error(
                codes::MALFORMED_TYPE,
                format!("{kind} name `{name}` collides with {what}; choose another name"),
                span,
            );
            return true;
        }
        if self.alias_lookup(name) {
            self.error(
                codes::MALFORMED_TYPE,
                format!(
                    "{kind} name `{name}` collides with a type alias of the same name; choose another name"
                ),
                span,
            );
            return true;
        }
        false
    }

    /// The declared variant set of a user enum, if `name` is one.
    pub(crate) fn enum_info(&self, name: &str) -> Option<&EnumInfo> {
        self.enums
            .get(name)
            .or_else(|| self.enums.values().find(|info| info.id == name))
    }

    pub(crate) fn enum_base_for_name(&self, name: &str) -> Option<String> {
        let info = self.enums.get(name).or_else(|| self.enum_info(name))?;
        let conflicting_alias = self.enums.iter().any(|(key, other)| {
            key != name && other.id == info.id && !enum_info_equivalent(other, info)
        });
        Some(if conflicting_alias {
            name.to_string()
        } else {
            info.id.clone()
        })
    }

    /// The full enum table (name → variant set), for payload-aware coverage of a
    /// NESTED enum payload (a `Coverage` over a `Type::Enum` payload type needs
    /// the variant set, which lives here).
    pub(crate) fn enum_table(&self) -> &HashMap<String, EnumInfo> {
        &self.enums
    }

    /// Whether `name` is a declared user enum.
    pub(crate) fn is_enum(&self, name: &str) -> bool {
        self.enums.contains_key(name)
    }

    /// The declared field set of a user nominal record, if `name` is one.
    pub(crate) fn record_info(&self, name: &str) -> Option<&RecordInfo> {
        self.records
            .get(name)
            .or_else(|| self.records.values().find(|info| info.id == name))
    }

    pub(crate) fn record_base_for_name(&self, name: &str) -> Option<String> {
        let info = self.records.get(name).or_else(|| self.record_info(name))?;
        let conflicting_alias = self.records.iter().any(|(key, other)| {
            key != name && other.id == info.id && !record_info_equivalent(other, info)
        });
        Some(if conflicting_alias {
            name.to_string()
        } else {
            info.id.clone()
        })
    }

    /// The full record table (name → field set), for field-type-aware reasoning
    /// over a `Type::NominalRecord` (e.g. comparability consults field types).
    pub(crate) fn record_table(&self) -> &HashMap<String, RecordInfo> {
        &self.records
    }

    /// Whether `name` is a declared user nominal record.
    pub(crate) fn is_record(&self, name: &str) -> bool {
        self.records.contains_key(name)
    }

    /// The declared base type of a user newtype, if `name` is one.
    pub(crate) fn newtype_info(&self, name: &str) -> Option<&NewtypeInfo> {
        self.newtypes
            .get(name)
            .or_else(|| self.newtypes.values().find(|info| info.id == name))
    }

    pub(crate) fn newtype_base_for_name(&self, name: &str) -> Option<String> {
        let info = self
            .newtypes
            .get(name)
            .or_else(|| self.newtype_info(name))?;
        let conflicting_alias = self.newtypes.iter().any(|(key, other)| {
            key != name && other.id == info.id && !newtype_info_equivalent(other, info)
        });
        Some(if conflicting_alias {
            name.to_string()
        } else {
            info.id.clone()
        })
    }

    /// Whether `name` is a declared user newtype.
    pub(crate) fn is_newtype(&self, name: &str) -> bool {
        self.newtypes.contains_key(name)
    }

    /// The full newtype table (name → base type), for base-type-aware reasoning
    /// over a `Type::Newtype` (e.g. comparability consults the base type).
    pub(crate) fn newtype_table(&self) -> &HashMap<String, NewtypeInfo> {
        &self.newtypes
    }

    pub(crate) fn enum_instance(
        &mut self,
        name: &str,
        formed_args: Vec<Type>,
        span: Span,
    ) -> Option<EnumInfo> {
        let template = self.enum_info(name)?.clone();
        let instance_base = self
            .enum_base_for_name(name)
            .unwrap_or_else(|| template.id.clone());
        self.instantiate_enum_template(name, &instance_base, template, formed_args, span)
    }

    pub(crate) fn record_instance(
        &mut self,
        name: &str,
        formed_args: Vec<Type>,
        span: Span,
    ) -> Option<RecordInfo> {
        let template = self.record_info(name)?.clone();
        let instance_base = self
            .record_base_for_name(name)
            .unwrap_or_else(|| template.id.clone());
        self.instantiate_record_template(name, &instance_base, template, formed_args, span)
    }

    pub(crate) fn newtype_instance(
        &mut self,
        name: &str,
        formed_args: Vec<Type>,
        span: Span,
    ) -> Option<NewtypeInfo> {
        let template = self.newtype_info(name)?.clone();
        let instance_base = self
            .newtype_base_for_name(name)
            .unwrap_or_else(|| template.id.clone());
        self.instantiate_newtype_template(name, &instance_base, template, formed_args, span)
    }

    pub(super) fn check_nominal_instance_args(
        &mut self,
        kind: &str,
        name: &str,
        params: &[String],
        formed_args: &[Type],
        span: Span,
    ) -> bool {
        if formed_args.len() != params.len() {
            self.error(
                codes::MALFORMED_TYPE,
                format!(
                    "{kind} `{name}` takes {} type argument{}, found {}",
                    params.len(),
                    if params.len() == 1 { "" } else { "s" },
                    formed_args.len()
                ),
                span,
            );
            return false;
        }
        if formed_args.iter().any(has_invalid_nominal_instance_arg) {
            self.error(
                codes::MALFORMED_TYPE,
                format!("{kind} `{name}` generic instantiation requires nameable type arguments"),
                span,
            );
            return false;
        }
        true
    }

    pub(super) fn instantiate_enum_template(
        &mut self,
        name: &str,
        instance_base: &str,
        template: EnumInfo,
        formed_args: Vec<Type>,
        span: Span,
    ) -> Option<EnumInfo> {
        if !self.check_nominal_instance_args(
            "enum",
            name,
            &template.type_params,
            &formed_args,
            span,
        ) {
            return None;
        }
        if formed_args.is_empty() {
            return Some(template);
        }
        let id = nominal_instance_id(instance_base, &formed_args);
        if !nominal_instance_args_have_scope_identity(&formed_args)
            && let Some(info) = self.enums.get(&id).cloned()
        {
            return Some(info);
        }
        let info = EnumInfo {
            id: id.clone(),
            type_params: Vec::new(),
            variants: template
                .variants
                .into_iter()
                .map(|variant| EnumVariantInfo {
                    name: variant.name,
                    payloads: variant
                        .payloads
                        .iter()
                        .map(|ty| substitute(ty, &formed_args))
                        .collect(),
                })
                .collect(),
        };
        self.enums.insert(id, info.clone());
        Some(info)
    }

    pub(super) fn instantiate_record_template(
        &mut self,
        name: &str,
        instance_base: &str,
        template: RecordInfo,
        formed_args: Vec<Type>,
        span: Span,
    ) -> Option<RecordInfo> {
        if !self.check_nominal_instance_args(
            "record",
            name,
            &template.type_params,
            &formed_args,
            span,
        ) {
            return None;
        }
        if formed_args.is_empty() {
            return Some(template);
        }
        let id = nominal_instance_id(instance_base, &formed_args);
        if !nominal_instance_args_have_scope_identity(&formed_args)
            && let Some(info) = self.records.get(&id).cloned()
        {
            return Some(info);
        }
        let info = RecordInfo {
            id: id.clone(),
            type_params: Vec::new(),
            fields: template
                .fields
                .into_iter()
                .map(|field| RecordFieldInfo {
                    name: field.name,
                    ty: substitute(&field.ty, &formed_args),
                    has_default: field.has_default,
                })
                .collect(),
        };
        self.records.insert(id, info.clone());
        Some(info)
    }

    pub(super) fn instantiate_newtype_template(
        &mut self,
        name: &str,
        instance_base: &str,
        template: NewtypeInfo,
        formed_args: Vec<Type>,
        span: Span,
    ) -> Option<NewtypeInfo> {
        if !self.check_nominal_instance_args(
            "newtype",
            name,
            &template.type_params,
            &formed_args,
            span,
        ) {
            return None;
        }
        if formed_args.is_empty() {
            return Some(template);
        }
        let id = nominal_instance_id(instance_base, &formed_args);
        if !nominal_instance_args_have_scope_identity(&formed_args)
            && let Some(info) = self.newtypes.get(&id).cloned()
        {
            return Some(info);
        }
        let info = NewtypeInfo {
            id: id.clone(),
            type_params: Vec::new(),
            base: substitute(&template.base, &formed_args),
        };
        self.newtypes.insert(id, info.clone());
        Some(info)
    }
}
