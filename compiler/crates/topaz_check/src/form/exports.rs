use super::*;

pub(super) fn exported_record_from_info(info: &RecordInfo) -> ExportedRecord {
    ExportedRecord {
        id: info.id.clone(),
        params: info.type_params.len(),
        fields: info
            .fields
            .iter()
            .map(|f| crate::unit::ExportedRecordField {
                name: f.name.clone(),
                ty: f.ty.clone(),
                has_default: f.has_default,
            })
            .collect(),
        nominals: ExportedNominals::default(),
    }
}

pub(super) fn exported_enum_from_info(info: &EnumInfo) -> ExportedEnum {
    ExportedEnum {
        id: info.id.clone(),
        params: info.type_params.len(),
        variants: info
            .variants
            .iter()
            .map(|v| crate::unit::ExportedEnumVariant {
                name: v.name.clone(),
                payloads: v.payloads.clone(),
            })
            .collect(),
        nominals: ExportedNominals::default(),
    }
}

pub(super) fn exported_newtype_from_info(info: &NewtypeInfo) -> ExportedNewtype {
    ExportedNewtype {
        id: info.id.clone(),
        params: info.type_params.len(),
        base: info.base.clone(),
        nominals: ExportedNominals::default(),
    }
}

pub(super) fn record_info_from_export(record: ExportedRecord) -> RecordInfo {
    RecordInfo {
        id: record.id,
        type_params: synthetic_type_params(record.params),
        fields: record
            .fields
            .into_iter()
            .map(|f| RecordFieldInfo {
                name: f.name,
                ty: f.ty,
                has_default: f.has_default,
            })
            .collect(),
    }
}

pub(super) fn enum_info_from_export(enm: ExportedEnum) -> EnumInfo {
    EnumInfo {
        id: enm.id,
        type_params: synthetic_type_params(enm.params),
        variants: enm
            .variants
            .into_iter()
            .map(|v| EnumVariantInfo {
                name: v.name,
                payloads: v.payloads,
            })
            .collect(),
    }
}

pub(super) fn enum_info_equivalent(left: &EnumInfo, right: &EnumInfo) -> bool {
    left.id == right.id
        && left.type_params.len() == right.type_params.len()
        && left.variants.len() == right.variants.len()
        && left
            .variants
            .iter()
            .zip(right.variants.iter())
            .all(|(a, b)| a.name == b.name && a.payloads == b.payloads)
}

pub(super) fn record_info_equivalent(left: &RecordInfo, right: &RecordInfo) -> bool {
    left.id == right.id
        && left.type_params.len() == right.type_params.len()
        && left.fields.len() == right.fields.len()
        && left
            .fields
            .iter()
            .zip(right.fields.iter())
            .all(|(a, b)| a.name == b.name && a.ty == b.ty && a.has_default == b.has_default)
}

pub(super) fn newtype_info_equivalent(left: &NewtypeInfo, right: &NewtypeInfo) -> bool {
    left.id == right.id
        && left.type_params.len() == right.type_params.len()
        && left.base == right.base
}

pub(super) fn newtype_info_from_export(newtype: ExportedNewtype) -> NewtypeInfo {
    NewtypeInfo {
        id: newtype.id,
        type_params: synthetic_type_params(newtype.params),
        base: newtype.base,
    }
}

impl<'a> Former<'a> {
    /// Enters module-aware mode with this module's import surface.
    pub(crate) fn set_module_context(&mut self, ctx: ModuleContext) {
        self.module_mode = true;
        self.namespace_aliases = ctx.namespace_aliases;
        self.namespace_records = ctx.namespace_records;
        self.namespace_enums = ctx.namespace_enums;
        self.namespace_newtypes = ctx.namespace_newtypes;
        self.imported_aliases = ctx.imported_aliases;
        self.imported_enum_sources = ctx.imported_enums.clone();
        self.imported_newtype_sources = ctx.imported_newtypes.clone();
        self.imported_schema_nominals
            .extend(ctx.imported_records.keys().cloned());
        self.imported_schema_nominals
            .extend(ctx.imported_enums.keys().cloned());
        self.imported_schema_nominals
            .extend(ctx.imported_newtypes.keys().cloned());
        self.ambient_namespaces = ctx.ambient_namespaces;
        for (protocol, type_id) in ctx.imported_conformances {
            catalog_insert_conformance(&mut self.conformances, protocol, type_id);
        }
        for (name, mut record) in ctx.imported_records {
            if self.version < LangVersion::V5_20 {
                record.id = name.clone();
            }
            let identity = record.id.clone();
            self.insert_exported_record(name, record.clone());
            if self.version >= LangVersion::V5_20 {
                self.insert_exported_record(identity, record);
            }
        }
        for (name, mut enm) in ctx.imported_enums {
            if self.version < LangVersion::V5_20 {
                enm.id = name.clone();
            }
            let identity = enm.id.clone();
            self.insert_exported_enum(name, enm.clone());
            if self.version >= LangVersion::V5_20 {
                self.insert_exported_enum(identity, enm);
            }
        }
        for (name, mut newtype) in ctx.imported_newtypes {
            if self.version < LangVersion::V5_20 {
                newtype.id = name.clone();
            }
            let identity = newtype.id.clone();
            self.insert_exported_newtype(name, newtype.clone());
            if self.version >= LangVersion::V5_20 {
                self.insert_exported_newtype(identity, newtype);
            }
        }
        for (nominal, methods) in ctx.imported_receiver_methods {
            self.install_exported_receiver_methods(nominal, methods);
        }
        let mut namespace_records = Vec::new();
        for (ns, table) in &self.namespace_records {
            for (name, record) in table {
                namespace_records.push((ns.clone(), name.clone(), record.clone()));
            }
        }
        namespace_records.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        for (ns, name, mut record) in namespace_records {
            if self.version < LangVersion::V5_20 {
                record.id = format!("{ns}.{name}");
                self.insert_exported_record(name.clone(), record.clone());
            }
            let identity = record.id.clone();
            self.insert_exported_record(format!("{ns}.{name}"), record.clone());
            if self.version >= LangVersion::V5_20 {
                self.insert_exported_record(identity, record);
            }
        }
        let mut namespace_enums = Vec::new();
        for (ns, table) in &self.namespace_enums {
            for (name, enm) in table {
                namespace_enums.push((ns.clone(), name.clone(), enm.clone()));
            }
        }
        namespace_enums.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        for (ns, name, mut enm) in namespace_enums {
            if self.version < LangVersion::V5_20 {
                enm.id = format!("{ns}.{name}");
                self.insert_exported_enum(name.clone(), enm.clone());
            }
            let identity = enm.id.clone();
            self.insert_exported_enum(format!("{ns}.{name}"), enm.clone());
            if self.version >= LangVersion::V5_20 {
                self.insert_exported_enum(identity, enm);
            }
        }
        let mut namespace_newtypes = Vec::new();
        for (ns, table) in &self.namespace_newtypes {
            for (name, newtype) in table {
                namespace_newtypes.push((ns.clone(), name.clone(), newtype.clone()));
            }
        }
        namespace_newtypes.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        for (ns, name, mut newtype) in namespace_newtypes {
            if self.version < LangVersion::V5_20 {
                newtype.id = format!("{ns}.{name}");
                self.insert_exported_newtype(name.clone(), newtype.clone());
            }
            let identity = newtype.id.clone();
            self.insert_exported_newtype(format!("{ns}.{name}"), newtype.clone());
            if self.version >= LangVersion::V5_20 {
                self.insert_exported_newtype(identity, newtype);
            }
        }
        for (ns, nominals) in ctx.namespace_receiver_methods {
            for (nominal, methods) in nominals {
                let checker_id = format!("{ns}.{nominal}");
                self.install_exported_receiver_methods(checker_id, methods);
            }
        }
    }

    pub(super) fn install_exported_receiver_methods(
        &mut self,
        checker_id: String,
        methods: HashMap<String, ExportedReceiverMethod>,
    ) {
        let canonical_id = methods
            .values()
            .next()
            .map(|method| method.dispatch_id.clone());
        let entries = methods
            .into_iter()
            .map(|(name, method)| {
                (
                    name,
                    InherentMethodInfo {
                        signature: method.info,
                        dispatch_id: Some(method.dispatch_id),
                    },
                )
            })
            .collect::<Vec<_>>();
        if self.version >= LangVersion::V5_20
            && let Some(canonical_id) = canonical_id
            && canonical_id != checker_id
        {
            self.methods
                .entry(canonical_id)
                .or_default()
                .extend(entries.iter().cloned());
        }
        self.methods.entry(checker_id).or_default().extend(entries);
    }

    /// Whether a materialized typed-JSON target crosses a module boundary.
    /// This remains the compatibility-profile rejection boundary through 5.19;
    /// 5.20 accepts the same AST after assigning ADR-131 declaration identity.
    pub(crate) fn json_schema_crosses_module(&self, ty: &ast::Type) -> bool {
        type_syntax_any(ty, &mut |component| match &component.kind {
            ast::TypeKind::Named { name, .. } => {
                self.imported_schema_nominals.contains(self.text(name.span))
                    || self.imported_aliases.contains_key(self.text(name.span))
            }
            ast::TypeKind::Qualified { .. } => true,
            _ => false,
        })
    }

    /// Typed JSON schema lowering materializes source type syntax at runtime.
    /// Root aliases have declaration-stable ASTs in both backends, but a nested
    /// block alias is lexical and the Rust emitter deliberately has no
    /// per-expression alias environment. Reject such a target at check time
    /// instead of accepting it and letting one backend resolve a different body.
    pub(crate) fn json_schema_uses_block_alias(&self, ty: &ast::Type) -> bool {
        type_syntax_any(ty, &mut |component| match &component.kind {
            ast::TypeKind::Named { name, .. } => {
                let head = self.text(name.span);
                self.aliases
                    .iter()
                    .skip(1)
                    .rev()
                    .any(|frame| frame.contains_key(head))
            }
            _ => false,
        })
    }

    pub(super) fn insert_exported_record(&mut self, name: String, record: ExportedRecord) {
        self.records
            .entry(name)
            .or_insert_with(|| record_info_from_export(record));
    }

    pub(super) fn insert_exported_enum(&mut self, name: String, enm: ExportedEnum) {
        self.enums
            .entry(name)
            .or_insert_with(|| enum_info_from_export(enm));
    }

    pub(super) fn insert_exported_newtype(&mut self, name: String, newtype: ExportedNewtype) {
        self.newtypes
            .entry(name)
            .or_insert_with(|| newtype_info_from_export(newtype));
    }

    pub(crate) fn collect_type_nominals(&self, ty: &Type, out: &mut ExportedNominals) {
        match ty {
            Type::NominalRecord { base, args } => {
                let id = nominal_instance_id(base, args);
                let Some(record) = self.record_info(&id) else {
                    return;
                };
                if out.records.contains_key(&id) {
                    return;
                }
                let exported = exported_record_from_info(record);
                let fields: Vec<Type> = exported.fields.iter().map(|f| f.ty.clone()).collect();
                out.records.insert(id, exported.clone());
                out.records
                    .entry(exported.id.clone())
                    .or_insert_with(|| exported.clone());
                for ty in fields {
                    self.collect_type_nominals(&ty, out);
                }
            }
            Type::Enum { base, args } => {
                let id = nominal_instance_id(base, args);
                let Some(enm) = self.enum_info(&id) else {
                    return;
                };
                if out.enums.contains_key(&id) {
                    return;
                }
                let mut exported = exported_enum_from_info(enm);
                if let Some(source_base) = self.imported_enum_source_base(base) {
                    exported.id = nominal_instance_id(source_base, args);
                }
                let payloads: Vec<Type> = exported
                    .variants
                    .iter()
                    .flat_map(|variant| variant.payloads.iter().cloned())
                    .collect();
                out.enums.insert(id, exported.clone());
                out.enums
                    .entry(exported.id.clone())
                    .or_insert_with(|| exported.clone());
                for ty in payloads {
                    self.collect_type_nominals(&ty, out);
                }
            }
            Type::Newtype { base, args } => {
                let id = nominal_instance_id(base, args);
                let Some(newtype) = self.newtype_info(&id) else {
                    return;
                };
                if out.newtypes.contains_key(&id) {
                    return;
                }
                let mut exported = exported_newtype_from_info(newtype);
                if let Some(source_base) = self.imported_newtype_source_base(base) {
                    exported.id = nominal_instance_id(source_base, args);
                }
                let base = exported.base.clone();
                out.newtypes.insert(id, exported.clone());
                out.newtypes
                    .entry(exported.id.clone())
                    .or_insert_with(|| exported.clone());
                self.collect_type_nominals(&base, out);
            }
            Type::Union(members) => {
                for member in members {
                    self.collect_type_nominals(member, out);
                }
            }
            Type::Record(fields) => {
                for (_, field) in fields {
                    self.collect_type_nominals(field, out);
                }
            }
            Type::Ctor(_, args) | Type::Foreign { args, .. } => {
                for arg in args {
                    self.collect_type_nominals(arg, out);
                }
            }
            Type::Func {
                params,
                variadic,
                ret,
            } => {
                for param in params {
                    self.collect_type_nominals(param, out);
                }
                if let Some(variadic) = variadic {
                    self.collect_type_nominals(variadic, out);
                }
                self.collect_type_nominals(ret, out);
            }
            Type::Prim(_)
            | Type::Literal(_)
            | Type::Template
            | Type::File
            | Type::JsonValue
            | Type::Bytes
            | Type::ByteBuffer
            | Type::Path
            | Type::Regex
            | Type::Match
            | Type::TomlValue
            | Type::Url
            | Type::Date
            | Type::BigInt
            | Type::Decimal
            | Type::RoundingMode
            | Type::Unknown
            | Type::Var(_)
            | Type::Skolem { .. } => {}
        }
    }

    pub(super) fn imported_enum_source_base(&self, checker_base: &str) -> Option<&str> {
        self.imported_enum_sources
            .get(checker_base)
            .map(|enm| enm.id.as_str())
            .or_else(|| {
                let (namespace, name) = checker_base.split_once('.')?;
                self.namespace_enums
                    .get(namespace)?
                    .get(name)
                    .map(|enm| enm.id.as_str())
            })
    }

    pub(super) fn imported_newtype_source_base(&self, checker_base: &str) -> Option<&str> {
        self.imported_newtype_sources
            .get(checker_base)
            .map(|newtype| newtype.id.as_str())
            .or_else(|| {
                let (namespace, name) = checker_base.split_once('.')?;
                self.namespace_newtypes
                    .get(namespace)?
                    .get(name)
                    .map(|newtype| newtype.id.as_str())
            })
    }

    /// An exported alias of this module's root frame, as a
    /// parameter count plus the resolved body (`Var(i)` holes).
    pub(crate) fn exported_alias(&self, name: &str) -> Option<(usize, Type)> {
        let def = self.aliases.first()?.get(name)?;
        Some((
            def.params.len(),
            def.resolved.clone().unwrap_or(Type::Unknown),
        ))
    }

    /// All aliases in this module's root frame, including private aliases.
    /// Backends consume this only after `validate_aliases`, so the bodies are the
    /// checker's resolved forms rather than a second backend-local resolver.
    pub(crate) fn root_aliases(&self, module_identity: &str) -> HashMap<String, ExportedAlias> {
        self.aliases
            .first()
            .map(|frame| {
                frame
                    .iter()
                    .map(|(name, def)| {
                        let body = def.resolved.clone().unwrap_or(Type::Unknown);
                        let mut nominals = ExportedNominals::default();
                        self.collect_type_nominals(&body, &mut nominals);
                        (
                            (*name).to_string(),
                            ExportedAlias {
                                defining_module: module_identity.to_string(),
                                params: def.params.len(),
                                body,
                                nominals,
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Instantiates an imported alias body with formed arguments.
    pub(super) fn apply_exported_alias(
        &mut self,
        display: &str,
        alias: &ExportedAlias,
        formed_args: Vec<Type>,
        span: Span,
    ) -> Type {
        if formed_args.len() != alias.params {
            self.error(
                codes::MALFORMED_TYPE,
                format!(
                    "`{display}` takes {} type argument{}, found {}",
                    alias.params,
                    if alias.params == 1 { "" } else { "s" },
                    formed_args.len()
                ),
                span,
            );
            return Type::Unknown;
        }
        substitute_params(&alias.body, &formed_args)
    }

    /// Projects declaration-local nominal identities through a namespace
    /// binding. Before 5.20, imported nominal tables use the namespace-qualified
    /// spelling; from 5.20 onward their module-stable identity is already the
    /// table result (or survives the fallback unchanged).
    pub(crate) fn qualify_namespace_type(&self, namespace: &str, ty: &Type) -> Type {
        ty.transform_components(&mut |component| match component {
            Type::Enum { base, args } => {
                let qualified = format!("{namespace}.{base}");
                self.enum_base_for_name(&qualified).map(|base| Type::Enum {
                    base,
                    args: args
                        .iter()
                        .map(|argument| self.qualify_namespace_type(namespace, argument))
                        .collect(),
                })
            }
            Type::NominalRecord { base, args } => {
                let qualified = format!("{namespace}.{base}");
                self.record_base_for_name(&qualified)
                    .map(|base| Type::NominalRecord {
                        base,
                        args: args
                            .iter()
                            .map(|argument| self.qualify_namespace_type(namespace, argument))
                            .collect(),
                    })
            }
            Type::Newtype { base, args } => {
                let qualified = format!("{namespace}.{base}");
                self.newtype_base_for_name(&qualified)
                    .map(|base| Type::Newtype {
                        base,
                        args: args
                            .iter()
                            .map(|argument| self.qualify_namespace_type(namespace, argument))
                            .collect(),
                    })
            }
            _ => None,
        })
    }
}
