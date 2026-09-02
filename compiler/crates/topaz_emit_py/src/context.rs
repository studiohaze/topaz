use crate::*;

#[derive(Clone)]
pub(super) struct Ctx<'a> {
    pub(super) map: &'a SourceMap,
    /// Method dispatch uses `__entry__` for the entry module while source and
    /// schema resolution keep the resolver identity (`main`). Imported modules
    /// use the same canonical identity for both roles.
    pub(super) method_module_identity: Option<&'a str>,
    pub(super) module_identity: &'a str,
    pub(super) schema_modules: Rc<JsonSchemaModules<'a>>,
    pub(super) records: Rc<std::collections::BTreeMap<String, NominalRecordDef<'a>>>,
    pub(super) newtypes: Rc<std::collections::BTreeMap<String, NewtypeDef>>,
    pub(super) enums: Rc<std::collections::BTreeMap<String, EnumDef>>,
    pub(super) type_aliases: Option<&'a BTreeMap<String, topaz_check::ExportedAlias>>,
    pub(super) functions: Rc<std::collections::BTreeMap<String, FunctionInfo>>,
    pub(super) receiver_methods: Rc<std::collections::BTreeMap<String, Vec<FunctionInfo>>>,
    pub(super) protocols: Rc<BTreeSet<String>>,
    pub(super) receiver_method_module_values: Rc<BTreeMap<String, String>>,
    pub(super) module_value_py_names: Rc<BTreeMap<String, String>>,
    pub(super) namespaces: Rc<std::collections::BTreeMap<String, ModuleRuntimeExports<'a>>>,
    pub(super) bindings: Vec<Rc<std::collections::BTreeMap<String, BindingInfo>>>,
    pub(super) binding_scope_ids: Vec<usize>,
    pub(super) loop_frames: Vec<LoopFrameKind>,
    pub(super) pipe_placeholders: RefCell<Vec<Rc<str>>>,
    pub(super) cooperative_yields: bool,
    pub(super) metadata_control_flow_depth: usize,
    pub(super) flow_static_metadata_blocked_names: Vec<Rc<BTreeSet<String>>>,
    pub(super) scope_counter: usize,
    pub(super) temp_counter: usize,
}

impl<'a> Ctx<'a> {
    pub(super) fn new(
        map: &'a SourceMap,
        records: impl Into<Rc<std::collections::BTreeMap<String, NominalRecordDef<'a>>>>,
        newtypes: impl Into<Rc<std::collections::BTreeMap<String, NewtypeDef>>>,
        enums: impl Into<Rc<std::collections::BTreeMap<String, EnumDef>>>,
        type_aliases: Option<&'a BTreeMap<String, topaz_check::ExportedAlias>>,
    ) -> Self {
        Self {
            map,
            method_module_identity: None,
            module_identity: "",
            schema_modules: Rc::new(BTreeMap::new()),
            records: records.into(),
            newtypes: newtypes.into(),
            enums: enums.into(),
            type_aliases,
            functions: Rc::new(std::collections::BTreeMap::new()),
            receiver_methods: Rc::new(std::collections::BTreeMap::new()),
            protocols: Rc::new(
                ["Show", "Eq", "Order"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            ),
            receiver_method_module_values: Rc::new(BTreeMap::new()),
            module_value_py_names: Rc::new(BTreeMap::new()),
            namespaces: Rc::new(std::collections::BTreeMap::new()),
            bindings: vec![Rc::new(std::collections::BTreeMap::new())],
            binding_scope_ids: vec![0],
            loop_frames: Vec::new(),
            pipe_placeholders: RefCell::new(Vec::new()),
            cooperative_yields: false,
            metadata_control_flow_depth: 0,
            flow_static_metadata_blocked_names: Vec::new(),
            scope_counter: 1,
            temp_counter: 0,
        }
    }

    pub(super) fn text(&self, span: Span) -> &'a str {
        text_in_map(self.map, span)
    }

    pub(super) fn type_alias(&self, source_name: &str) -> Option<&topaz_check::ExportedAlias> {
        self.type_aliases?.get(source_name)
    }

    pub(super) fn has_type_alias(&self, source_name: &str) -> bool {
        self.type_aliases
            .is_some_and(|aliases| aliases.contains_key(source_name))
    }

    pub(super) fn binding_name(&self, pattern: &Pattern) -> Result<&'a str, PyEmitError> {
        binding_name(pattern, self.map)
    }

    pub(super) fn register_function_info(&mut self, source_name: &str, info: FunctionInfo) {
        Rc::make_mut(&mut self.functions).insert(source_name.to_string(), info);
    }

    pub(super) fn register_receiver_method_info(&mut self, source_name: &str, info: FunctionInfo) {
        Rc::make_mut(&mut self.receiver_methods)
            .entry(source_name.to_string())
            .or_default()
            .push(info);
    }

    pub(super) fn receiver_method_info(&self, source_name: &str) -> Option<&FunctionInfo> {
        let infos = self.receiver_methods.get(source_name)?;
        let first = infos.first()?;
        infos
            .iter()
            .all(|info| info.params == first.params)
            .then_some(first)
    }

    pub(super) fn receiver_method_known(&self, source_name: &str) -> bool {
        self.receiver_methods.contains_key(source_name)
    }

    pub(super) fn receiver_method_module_value_py_name(&self, source_name: &str) -> Option<&str> {
        self.binding_lookup(source_name)
            .is_none_or(|(scope, _)| scope == 0)
            .then(|| self.receiver_method_module_values.get(source_name))
            .flatten()
            .map(String::as_str)
    }

    pub(super) fn enrich_function_return_metadata(
        &mut self,
        source_name: &str,
        decl: &FunctionDecl,
    ) -> bool {
        let Some(tail) = metadata_join_block_tail_expr(decl.body.as_ref()) else {
            return false;
        };
        self.push_scope();
        for param in &decl.params {
            let raw = self.text(param.name.span).to_string();
            if param.variadic {
                self.register_array_binding(&raw, false);
            } else {
                self.register_typed_binding(&raw, false, &param.ty);
            }
        }
        let mut return_record_descendants = decl
            .return_type
            .as_ref()
            .map(|ty| record_descendant_catalog_from_type(ty, self))
            .unwrap_or_default();
        return_record_descendants.extend(self.record_descendant_catalog_for_value(tail, false));
        let return_shape = receiver_shape_from_value(tail, self);
        let observed_wrapped_metadata = self.wrapped_value_metadata_catalog_for_value(tail);
        self.pop_scope();
        let Some(info) = Rc::make_mut(&mut self.functions).get_mut(source_name) else {
            return false;
        };
        let next_return_shape = return_shape.or(info.return_shape);
        let mut next_wrapped_metadata = info.return_wrapped_metadata.clone();
        next_wrapped_metadata.overlay(observed_wrapped_metadata);
        let changed = info.return_shape != next_return_shape
            || info.return_wrapped_metadata != next_wrapped_metadata
            || info.return_record_descendants != return_record_descendants;
        info.return_shape = next_return_shape;
        info.return_wrapped_metadata = next_wrapped_metadata;
        info.return_record_descendants = return_record_descendants;
        changed
    }

    pub(super) fn register_binding(&mut self, source_name: &str, mutable: bool) {
        self.current_binding_scope_mut().insert(
            source_name.to_string(),
            BindingInfo {
                py_name: None,
                forward_function_cell: false,
                cooperative_py_name: None,
                cooperative_callback_py_name: None,
                cooperative_callback_needs_host: false,
                array_elements: ArrayElementMetadata::default(),
                declared_record_descendants: RecordDescendantCatalog::default(),
                record_descendants: RecordDescendantCatalog::default(),
                map_value: MapValueMetadata::default(),
                namespace_member_value_metadata: false,
                collection_storage_identity: None,
                mutable,
                namespace_import: false,
                typed_rebind_callable_params: None,
                callable_params: None,
                mutated_collection_params: BTreeSet::new(),
                callable_params_flow_allowed: false,
                composed: false,
                string: false,
                template: false,
                array: false,
                map: false,
                bytes: false,
                byte_buffer: false,
                json: false,
                option: false,
                result: false,
                wrapped_value_metadata: WrappedValueMetadataCatalog::default(),
            },
        );
    }

    pub(super) fn register_binding_with_callable_params(
        &mut self,
        source_name: &str,
        params: Vec<FunctionParamInfo>,
    ) {
        self.register_binding(source_name, false);
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.typed_rebind_callable_params = Some(params.clone());
            info.callable_params = Some(params);
            info.callable_params_flow_allowed = true;
        }
    }

    pub(super) fn set_binding_receiver_shape(
        &mut self,
        source_name: &str,
        shape: Option<ReceiverShape>,
        wrapped_value_metadata: WrappedValueMetadataCatalog,
    ) {
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.string = shape == Some(ReceiverShape::String);
            info.template = shape == Some(ReceiverShape::Template);
            info.array = shape == Some(ReceiverShape::Array);
            info.map = shape == Some(ReceiverShape::Map);
            info.bytes = shape == Some(ReceiverShape::Bytes);
            info.byte_buffer = shape == Some(ReceiverShape::ByteBuffer);
            info.json = shape == Some(ReceiverShape::Json);
            info.option = shape == Some(ReceiverShape::Option);
            info.result = shape == Some(ReceiverShape::Result);
            info.wrapped_value_metadata = wrapped_value_metadata;
        }
    }

    pub(super) fn set_binding_descendant_metadata(
        &mut self,
        source_name: &str,
        record_descendants: RecordDescendantCatalog,
    ) {
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.declared_record_descendants = record_descendants;
        }
    }

    pub(super) fn register_namespace_binding(&mut self, source_name: &str) {
        self.current_binding_scope_mut().insert(
            source_name.to_string(),
            BindingInfo {
                py_name: None,
                forward_function_cell: false,
                cooperative_py_name: None,
                cooperative_callback_py_name: None,
                cooperative_callback_needs_host: false,
                array_elements: ArrayElementMetadata::default(),
                declared_record_descendants: RecordDescendantCatalog::default(),
                record_descendants: RecordDescendantCatalog::default(),
                map_value: MapValueMetadata::default(),
                namespace_member_value_metadata: false,
                collection_storage_identity: None,
                mutable: false,
                namespace_import: true,
                typed_rebind_callable_params: None,
                callable_params: None,
                mutated_collection_params: BTreeSet::new(),
                callable_params_flow_allowed: false,
                composed: false,
                string: false,
                template: false,
                array: false,
                map: false,
                bytes: false,
                byte_buffer: false,
                json: false,
                option: false,
                result: false,
                wrapped_value_metadata: WrappedValueMetadataCatalog::default(),
            },
        );
    }

    pub(super) fn register_imported_value_binding(
        &mut self,
        source_name: &str,
        py_name: &str,
        cooperative_callback_target: Option<(String, bool)>,
        metadata: ModuleValueMetadata,
    ) {
        let callable_params_flow_allowed = metadata.callable_params.is_some();
        let receiver_shape = metadata.receiver_shape;
        self.current_binding_scope_mut().insert(
            source_name.to_string(),
            BindingInfo {
                py_name: Some(py_name.to_string()),
                forward_function_cell: false,
                cooperative_py_name: None,
                cooperative_callback_py_name: cooperative_callback_target
                    .as_ref()
                    .map(|target| target.0.clone()),
                cooperative_callback_needs_host: cooperative_callback_target
                    .as_ref()
                    .is_some_and(|target| target.1),
                array_elements: metadata.array_elements,
                declared_record_descendants: RecordDescendantCatalog::default(),
                record_descendants: metadata.record_descendants,
                map_value: metadata.map_value,
                namespace_member_value_metadata: false,
                collection_storage_identity: is_mutable_collection_shape(receiver_shape)
                    .then(|| CollectionStorageIdentity::Namespace(py_name.to_string())),
                mutable: false,
                namespace_import: false,
                typed_rebind_callable_params: None,
                callable_params: metadata.callable_params,
                mutated_collection_params: metadata.mutated_collection_params,
                callable_params_flow_allowed,
                composed: false,
                string: receiver_shape == Some(ReceiverShape::String),
                template: receiver_shape == Some(ReceiverShape::Template),
                array: receiver_shape == Some(ReceiverShape::Array),
                map: receiver_shape == Some(ReceiverShape::Map),
                bytes: receiver_shape == Some(ReceiverShape::Bytes),
                byte_buffer: receiver_shape == Some(ReceiverShape::ByteBuffer),
                json: receiver_shape == Some(ReceiverShape::Json),
                option: receiver_shape == Some(ReceiverShape::Option),
                result: receiver_shape == Some(ReceiverShape::Result),
                wrapped_value_metadata: metadata.wrapped_value_metadata,
            },
        );
    }

    pub(super) fn register_array_binding(&mut self, source_name: &str, mutable: bool) {
        self.current_binding_scope_mut().insert(
            source_name.to_string(),
            BindingInfo {
                py_name: None,
                forward_function_cell: false,
                cooperative_py_name: None,
                cooperative_callback_py_name: None,
                cooperative_callback_needs_host: false,
                array_elements: ArrayElementMetadata::default(),
                declared_record_descendants: RecordDescendantCatalog::default(),
                record_descendants: RecordDescendantCatalog::default(),
                map_value: MapValueMetadata::default(),
                namespace_member_value_metadata: false,
                collection_storage_identity: None,
                mutable,
                namespace_import: false,
                typed_rebind_callable_params: None,
                callable_params: None,
                mutated_collection_params: BTreeSet::new(),
                callable_params_flow_allowed: false,
                composed: false,
                string: false,
                template: false,
                array: true,
                map: false,
                bytes: false,
                byte_buffer: false,
                json: false,
                option: false,
                result: false,
                wrapped_value_metadata: WrappedValueMetadataCatalog::default(),
            },
        );
    }

    pub(super) fn register_typed_binding(&mut self, source_name: &str, mutable: bool, ty: &Type) {
        let declared_shape = receiver_shape_from_type(ty, self);
        let wrapped_value_metadata = wrapped_value_metadata_catalog_from_type(ty, self);
        let declared_record_descendants = record_descendant_catalog_from_type(ty, self);
        let map_value = map_value_metadata_from_type(ty, self);
        let callable_params = function_callable_params_from_type(ty, self);
        let callable_params_flow_allowed = callable_params.is_some();
        let array_elements = array_element_metadata_from_type(ty, self);
        self.current_binding_scope_mut().insert(
            source_name.to_string(),
            BindingInfo {
                py_name: None,
                forward_function_cell: false,
                cooperative_py_name: None,
                cooperative_callback_py_name: None,
                cooperative_callback_needs_host: false,
                array_elements,
                declared_record_descendants,
                record_descendants: RecordDescendantCatalog::default(),
                map_value,
                namespace_member_value_metadata: false,
                collection_storage_identity: None,
                mutable,
                namespace_import: false,
                typed_rebind_callable_params: None,
                callable_params,
                mutated_collection_params: BTreeSet::new(),
                callable_params_flow_allowed,
                composed: false,
                string: declared_shape == Some(ReceiverShape::String),
                template: declared_shape == Some(ReceiverShape::Template),
                array: declared_shape == Some(ReceiverShape::Array),
                map: declared_shape == Some(ReceiverShape::Map),
                bytes: declared_shape == Some(ReceiverShape::Bytes),
                byte_buffer: declared_shape == Some(ReceiverShape::ByteBuffer),
                json: declared_shape == Some(ReceiverShape::Json),
                option: declared_shape == Some(ReceiverShape::Option),
                result: declared_shape == Some(ReceiverShape::Result),
                wrapped_value_metadata,
            },
        );
    }

    pub(super) fn register_checked_binding(
        &mut self,
        source_name: &str,
        mutable: bool,
        ty: &CheckType,
    ) {
        self.register_binding(source_name, mutable);
        let declared_shape = receiver_shape_from_checked_type(ty);
        let wrapped_value_metadata = wrapped_value_metadata_catalog_from_checked_type(ty);
        let callable_params = match ty {
            CheckType::Func {
                params, variadic, ..
            } => Some(checked_function_type_param_info(
                params,
                variadic.as_deref(),
            )),
            _ => None,
        };
        let array_elements = array_element_metadata_from_checked_type(ty);
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.callable_params_flow_allowed = callable_params.is_some();
            info.callable_params = callable_params;
            info.array_elements = array_elements;
            info.string = declared_shape == Some(ReceiverShape::String);
            info.array = declared_shape == Some(ReceiverShape::Array);
            info.map = declared_shape == Some(ReceiverShape::Map);
            info.bytes = declared_shape == Some(ReceiverShape::Bytes);
            info.byte_buffer = declared_shape == Some(ReceiverShape::ByteBuffer);
            info.json = declared_shape == Some(ReceiverShape::Json);
            info.option = declared_shape == Some(ReceiverShape::Option);
            info.result = declared_shape == Some(ReceiverShape::Result);
            info.wrapped_value_metadata = wrapped_value_metadata;
        }
    }

    pub(super) fn register_value_binding(
        &mut self,
        source_name: &str,
        mutable: bool,
        value: &Expr,
        ty: Option<&Type>,
        cooperative_callback_target_override: Option<(String, bool)>,
    ) {
        let declared_shape = ty.and_then(|ty| receiver_shape_from_type(ty, self));
        let string = declared_shape == Some(ReceiverShape::String) || string_value(value, self);
        let template =
            declared_shape == Some(ReceiverShape::Template) || template_value(value, self);
        let array = declared_shape == Some(ReceiverShape::Array) || array_value(value, self);
        let option = declared_shape == Some(ReceiverShape::Option) || option_value(value, self);
        let result = declared_shape == Some(ReceiverShape::Result) || result_value(value, self);
        let mut wrapped_value_metadata = ty
            .map(|ty| wrapped_value_metadata_catalog_from_type(ty, self))
            .unwrap_or_default();
        wrapped_value_metadata.overlay(self.wrapped_value_metadata_catalog_for_value(value));
        let map = declared_shape == Some(ReceiverShape::Map) || map_value(value, self);
        let bytes = declared_shape == Some(ReceiverShape::Bytes) || bytes_value(value, self);
        let byte_buffer =
            declared_shape == Some(ReceiverShape::ByteBuffer) || byte_buffer_value(value, self);
        let json = declared_shape == Some(ReceiverShape::Json) || json_value(value, self);
        let composed = compose_binding_value(value, self);
        let namespace_member_value_metadata =
            self.namespace_member_value_metadata_origin_for_value(value);
        let collection_storage_identity =
            self.collection_storage_identity_for_binding_value(value, mutable);
        let mutable_namespace_member_value_metadata = mutable && namespace_member_value_metadata;
        let mutable_tracked_collection_metadata = mutable && collection_storage_identity.is_some();
        let mutable_stable_value_metadata =
            mutable_namespace_member_value_metadata || mutable_tracked_collection_metadata;
        let declared_callable_params =
            ty.and_then(|ty| function_callable_params_from_type(ty, self));
        let typed_rebind_callable_params = if declared_callable_params.is_some() {
            self.typed_rebind_callable_params_for_value(value)
        } else {
            None
        };
        let (callable_params, callable_params_flow_allowed) =
            if mutable && !mutable_stable_value_metadata {
                (None, false)
            } else {
                let flow_callable_params = callable_param_info(value, self)
                    .or(typed_rebind_callable_params)
                    .or(declared_callable_params);
                if flow_callable_params.is_some() {
                    (flow_callable_params, true)
                } else {
                    (direct_call_callable_param_info(value, self), false)
                }
            };
        let metadata_lookup_mutable = mutable && !mutable_stable_value_metadata;
        let cooperative_callback_target = cooperative_callback_target_override
            .or_else(|| self.cooperative_callback_target_for_value(value, metadata_lookup_mutable));
        let mut array_elements = ty
            .map(|ty| array_element_metadata_from_type(ty, self))
            .unwrap_or_default();
        array_elements.replace_observations(self.array_element_observations_for_value(
            value,
            ArrayElementObservationPolicy::binding_registration(metadata_lookup_mutable),
        ));
        let declared_record_descendants = ty
            .map(|ty| record_descendant_catalog_from_type(ty, self))
            .unwrap_or_default();
        let record_descendants =
            self.record_descendant_catalog_for_value(value, metadata_lookup_mutable);
        let record_descendants = if mutable_namespace_member_value_metadata {
            record_descendants.direct_fields_only()
        } else {
            record_descendants
        };
        let mut map_value = ty
            .map(|ty| map_value_metadata_from_type(ty, self))
            .unwrap_or_default();
        let observed_map_value = self.map_value_metadata_for_value(value, metadata_lookup_mutable);
        map_value.observed_by_key = observed_map_value.observed_by_key;
        map_value.known_present_keys = observed_map_value.known_present_keys;
        map_value.observed_keys_complete = observed_map_value.observed_keys_complete;
        let mutated_collection_params = if callable_params_flow_allowed {
            self.callable_mutated_collection_params_for_value_with_type(value, ty)
        } else {
            BTreeSet::new()
        };
        self.current_binding_scope_mut().insert(
            source_name.to_string(),
            BindingInfo {
                py_name: None,
                forward_function_cell: false,
                cooperative_py_name: None,
                cooperative_callback_py_name: cooperative_callback_target
                    .as_ref()
                    .map(|target| target.0.clone()),
                cooperative_callback_needs_host: cooperative_callback_target
                    .as_ref()
                    .is_some_and(|target| target.1),
                array_elements,
                declared_record_descendants,
                record_descendants,
                map_value,
                namespace_member_value_metadata,
                collection_storage_identity,
                mutable,
                namespace_import: false,
                typed_rebind_callable_params: None,
                callable_params,
                mutated_collection_params,
                callable_params_flow_allowed,
                composed,
                string,
                template,
                array,
                map,
                bytes,
                byte_buffer,
                json,
                option,
                result,
                wrapped_value_metadata,
            },
        );
    }

    pub(super) fn register_callable_binding(
        &mut self,
        source_name: &str,
        params: Vec<FunctionParamInfo>,
        mutated_collection_params: BTreeSet<usize>,
        cooperative_py_name: Option<String>,
        forward_function_cell: bool,
    ) {
        self.current_binding_scope_mut().insert(
            source_name.to_string(),
            BindingInfo {
                py_name: None,
                forward_function_cell,
                cooperative_py_name: cooperative_py_name.clone(),
                cooperative_callback_py_name: cooperative_py_name,
                cooperative_callback_needs_host: false,
                array_elements: ArrayElementMetadata::default(),
                declared_record_descendants: RecordDescendantCatalog::default(),
                record_descendants: RecordDescendantCatalog::default(),
                map_value: MapValueMetadata::default(),
                namespace_member_value_metadata: false,
                collection_storage_identity: None,
                mutable: false,
                namespace_import: false,
                typed_rebind_callable_params: None,
                callable_params: Some(params),
                mutated_collection_params,
                callable_params_flow_allowed: true,
                composed: false,
                string: false,
                template: false,
                array: false,
                map: false,
                bytes: false,
                byte_buffer: false,
                json: false,
                option: false,
                result: false,
                wrapped_value_metadata: WrappedValueMetadataCatalog::default(),
            },
        );
    }

    pub(super) fn extend_callable_binding_mutated_collection_params(
        &mut self,
        source_name: &str,
        params: BTreeSet<usize>,
    ) -> bool {
        let Some((_, info)) = self.binding_lookup_mut(source_name) else {
            return false;
        };
        let previous_len = info.mutated_collection_params.len();
        info.mutated_collection_params.extend(params);
        info.mutated_collection_params.len() != previous_len
    }

    pub(super) fn binding_is_mutable(&self, source_name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .find_map(|scope| scope.get(source_name).map(|info| info.mutable))
            .unwrap_or(false)
    }

    pub(super) fn set_binding_py_name(&mut self, source_name: &str, py_name: String) {
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.py_name = Some(py_name);
        }
    }

    pub(super) fn binding_lookup_mut(
        &mut self,
        source_name: &str,
    ) -> Option<(usize, &mut BindingInfo)> {
        let index = (0..self.bindings.len())
            .rev()
            .find(|index| self.bindings[*index].contains_key(source_name))?;
        let scope = Rc::make_mut(&mut self.bindings[index]);
        let info = scope
            .get_mut(source_name)
            .expect("binding found before mutable scope access");
        Some((index, info))
    }

    pub(super) fn current_binding_scope_mut(
        &mut self,
    ) -> &mut std::collections::BTreeMap<String, BindingInfo> {
        let scope = self.bindings.last_mut().expect("binding scope");
        Rc::make_mut(scope)
    }

    pub(super) fn clear_array_element_observations(&mut self, source_name: &str) {
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.array_elements.clear_observations();
            info.namespace_member_value_metadata = false;
        }
    }
}

impl<'a> Ctx<'a> {
    pub(super) fn cooperative_callback_target_for_value(
        &self,
        value: &Expr,
        mutable: bool,
    ) -> Option<(String, bool)> {
        if mutable {
            return None;
        }
        match &value.kind {
            ExprKind::Ident => {
                let name = self.text(value.span);
                if self.binding_is_bound(name) {
                    self.binding_cooperative_callback_target(name, value.span)
                } else {
                    self.function_info(name).and_then(|info| {
                        info.cooperative_py_name
                            .as_ref()
                            .map(|py_name| (py_name.clone(), info.needs_host))
                    })
                }
            }
            ExprKind::Member { .. } => {
                self.namespace_value_cooperative_callback_target_for_member_expr(value)
            }
            ExprKind::Paren(inner) => self.cooperative_callback_target_for_value(inner, mutable),
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => join_identical_if_branch_metadata(then_block, else_branch.as_deref(), |branch| {
                self.cooperative_callback_target_for_value(branch, mutable)
            })
            .flatten(),
            ExprKind::Match { cases, .. } => join_identical_match_arm_metadata(cases, |arm| {
                self.cooperative_callback_target_for_value(arm, mutable)
            })
            .flatten(),
            _ => None,
        }
    }

    pub(super) fn assignment_target_py_name(&self, source_name: &str) -> String {
        if let Some((scope_index, info)) = self.binding_lookup(source_name) {
            let py_name = info.py_name.clone().unwrap_or_else(|| mangle(source_name));
            if scope_index == 0 {
                format!("globals()[{}]", py_string(&py_name))
            } else {
                py_name
            }
        } else {
            mangle(source_name)
        }
    }

    pub(super) fn nonlocal_py_name_for_assignment(&self, source_name: &str) -> Option<String> {
        self.binding_lookup(source_name)
            .and_then(|(scope_index, info)| {
                if scope_index == 0 {
                    None
                } else {
                    Some(info.py_name.clone().unwrap_or_else(|| mangle(source_name)))
                }
            })
    }

    pub(super) fn new_binding_py_name(&self, source_name: &str) -> String {
        let py_name = mangle(source_name);
        if self.bindings.len() > 1 && self.name_visible_outside_current_scope(source_name) {
            format!("{py_name}__s{}", self.current_scope_id())
        } else {
            py_name
        }
    }

    pub(super) fn new_binding_target_py_name(&self, py_name: &str) -> String {
        if self.bindings.len() == 1 {
            format!("globals()[{}]", py_string(py_name))
        } else {
            py_name.to_string()
        }
    }

    pub(super) fn binding_callable_info_inner(
        &self,
        source_name: &str,
        require_flow_allowed: bool,
    ) -> Option<FunctionInfo> {
        self.bindings.iter().rev().find_map(|scope| {
            scope.get(source_name).and_then(|info| {
                if !self.binding_allows_value_static_metadata(source_name, info) {
                    return None;
                }
                if require_flow_allowed && !info.callable_params_flow_allowed {
                    return None;
                }
                info.callable_params.as_ref().map(|params| FunctionInfo {
                    py_name: info.py_name.clone().unwrap_or_else(|| mangle(source_name)),
                    cooperative_py_name: info.cooperative_py_name.clone(),
                    params: params.clone(),
                    return_shape: None,
                    return_wrapped_metadata: WrappedValueMetadataCatalog::default(),
                    return_record_descendants: RecordDescendantCatalog::default(),
                    mutated_collection_params: info.mutated_collection_params.clone(),
                    needs_host: false,
                })
            })
        })
    }

    pub(super) fn binding_callable_info(&self, source_name: &str) -> Option<FunctionInfo> {
        self.binding_callable_info_inner(source_name, false)
    }

    pub(super) fn function_effect_info_for_callee(&self, callee: &Expr) -> Option<FunctionInfo> {
        match &callee.kind {
            ExprKind::Ident => {
                let name = self.text(callee.span);
                if self.binding_is_bound(name) {
                    self.binding_callable_info(name)
                } else {
                    self.function_info(name).cloned()
                }
            }
            ExprKind::Member { object, field } => {
                let ExprKind::Ident = &object.kind else {
                    return None;
                };
                let namespace = self.text(object.span);
                match self.namespace_export(namespace, self.text(field.span))? {
                    ModuleRuntimeExport::Function { info } => Some(info.clone()),
                    ModuleRuntimeExport::Value {
                        py_name,
                        cooperative_callback,
                        metadata,
                    } => Some(FunctionInfo {
                        py_name: py_name.clone(),
                        cooperative_py_name: cooperative_callback
                            .as_ref()
                            .map(|target| target.0.clone()),
                        params: metadata.callable_params.clone()?,
                        return_shape: None,
                        return_wrapped_metadata: WrappedValueMetadataCatalog::default(),
                        return_record_descendants: RecordDescendantCatalog::default(),
                        mutated_collection_params: metadata.mutated_collection_params.clone(),
                        needs_host: false,
                    }),
                    _ => None,
                }
            }
            ExprKind::Paren(inner) => self.function_effect_info_for_callee(inner),
            _ => None,
        }
    }

    pub(super) fn callable_mutated_collection_params_for_value(
        &self,
        value: &Expr,
    ) -> BTreeSet<usize> {
        self.callable_mutated_collection_params_for_value_with_type(value, None)
    }

    pub(super) fn callable_mutated_collection_params_for_value_with_type(
        &self,
        value: &Expr,
        contextual_ty: Option<&Type>,
    ) -> BTreeSet<usize> {
        match &value.kind {
            ExprKind::Lambda { params, body } => {
                mutated_lambda_collection_parameter_indices(params, body, contextual_ty, self)
            }
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => {
                let Some(then_expr) = metadata_join_block_tail_expr(then_block) else {
                    return BTreeSet::new();
                };
                let Some(else_expr) = else_branch.as_deref().and_then(metadata_join_else_expr)
                else {
                    return BTreeSet::new();
                };
                let mut effects = self.callable_mutated_collection_params_for_value_with_type(
                    then_expr,
                    contextual_ty,
                );
                effects.extend(self.callable_mutated_collection_params_for_value_with_type(
                    else_expr,
                    contextual_ty,
                ));
                effects
            }
            ExprKind::Match { cases, .. }
                if cases.last().is_some_and(match_case_is_unguarded_catch_all) =>
            {
                let mut effects = BTreeSet::new();
                for case in cases {
                    let Some(body) = metadata_join_match_body_expr(&case.body) else {
                        return BTreeSet::new();
                    };
                    effects.extend(self.callable_mutated_collection_params_for_value_with_type(
                        body,
                        contextual_ty,
                    ));
                }
                effects
            }
            ExprKind::Paren(inner) => {
                self.callable_mutated_collection_params_for_value_with_type(inner, contextual_ty)
            }
            _ => self
                .function_effect_info_for_callee(value)
                .map(|info| info.mutated_collection_params)
                .unwrap_or_default(),
        }
    }

    pub(super) fn binding_callable_info_at(
        &self,
        source_name: &str,
        span: Span,
    ) -> Option<FunctionInfo> {
        let mut callable = self.binding_callable_info(source_name)?;
        if self.binding_is_forward_function_cell(source_name) {
            callable.py_name = self.forward_function_value_py(source_name, &callable.py_name, span);
            callable.cooperative_py_name = callable
                .cooperative_py_name
                .as_deref()
                .map(|py_name| self.forward_function_value_py(source_name, py_name, span));
        }
        Some(callable)
    }

    pub(super) fn binding_flow_callable_info(&self, source_name: &str) -> Option<FunctionInfo> {
        self.binding_callable_info_inner(source_name, true)
    }

    pub(super) fn binding_is_composed(&self, source_name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .find_map(|scope| {
                scope.get(source_name).map(|info| {
                    self.binding_allows_flow_static_metadata(source_name, info) && info.composed
                })
            })
            .unwrap_or(false)
    }

    pub(super) fn binding_is_bound(&self, source_name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .any(|scope| scope.contains_key(source_name))
            || self.module_value_py_names.contains_key(source_name)
    }

    pub(super) fn binding_lookup(&self, source_name: &str) -> Option<(usize, &BindingInfo)> {
        self.bindings
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, scope)| scope.get(source_name).map(|info| (index, info)))
    }

    pub(super) fn current_binding_info(&self, source_name: &str) -> Option<BindingInfo> {
        self.bindings
            .last()
            .and_then(|scope| scope.get(source_name).cloned())
    }

    pub(super) fn module_value_metadata_for_export(
        &self,
        source_name: &str,
    ) -> ModuleValueMetadata {
        let Some(info) = self.current_binding_info(source_name) else {
            return ModuleValueMetadata::empty();
        };
        if info.mutable {
            return ModuleValueMetadata::empty();
        }
        let receiver_shape = binding_receiver_shape(&info);
        let callable_params = if info.callable_params_flow_allowed {
            info.callable_params
        } else {
            None
        };
        let mut record_descendants = info.declared_record_descendants;
        record_descendants.extend(info.record_descendants);
        ModuleValueMetadata {
            callable_params,
            mutated_collection_params: info.mutated_collection_params,
            array_elements: info.array_elements,
            record_descendants,
            map_value: info.map_value,
            receiver_shape,
            wrapped_value_metadata: info.wrapped_value_metadata,
        }
    }

    pub(super) fn current_scope_contains(&self, source_name: &str) -> bool {
        self.bindings
            .last()
            .is_some_and(|scope| scope.contains_key(source_name))
    }

    pub(super) fn in_metadata_control_flow(&self) -> bool {
        self.metadata_control_flow_depth > 0
    }

    pub(super) fn binding_allows_flow_static_metadata(
        &self,
        source_name: &str,
        info: &BindingInfo,
    ) -> bool {
        !info.mutable
            || (!self.in_metadata_control_flow()
                && !self.flow_static_metadata_name_is_blocked(source_name))
    }

    pub(super) fn binding_allows_value_static_metadata(
        &self,
        source_name: &str,
        info: &BindingInfo,
    ) -> bool {
        self.binding_allows_flow_static_metadata(source_name, info)
            || (info.namespace_member_value_metadata
                && !self.flow_static_metadata_name_is_blocked(source_name))
    }

    pub(super) fn with_metadata_control_flow<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.metadata_control_flow_depth += 1;
        let result = f(self);
        self.metadata_control_flow_depth -= 1;
        result
    }

    pub(super) fn flow_static_metadata_name_is_blocked(&self, source_name: &str) -> bool {
        self.flow_static_metadata_blocked_names
            .iter()
            .rev()
            .any(|names| names.contains(source_name))
    }

    pub(super) fn with_flow_static_metadata_blocked_names<T>(
        &mut self,
        names: BTreeSet<String>,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        if names.is_empty() {
            return f(self);
        }
        self.flow_static_metadata_blocked_names.push(Rc::new(names));
        let result = f(self);
        self.flow_static_metadata_blocked_names.pop();
        result
    }

    pub(super) fn restore_current_binding(
        &mut self,
        source_name: String,
        previous: Option<BindingInfo>,
    ) {
        let scope = self.current_binding_scope_mut();
        match previous {
            Some(info) => {
                scope.insert(source_name, info);
            }
            None => {
                scope.remove(&source_name);
            }
        }
    }

    pub(super) fn current_scope_id(&self) -> usize {
        *self.binding_scope_ids.last().expect("binding scope id")
    }

    pub(super) fn name_visible_outside_current_scope(&self, source_name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .skip(1)
            .any(|scope| scope.contains_key(source_name))
            || self.functions.contains_key(source_name)
            || self.namespaces.contains_key(source_name)
            || self.records.contains_key(source_name)
            || self.newtypes.contains_key(source_name)
            || self.enums.contains_key(source_name)
    }

    pub(super) fn binding_is_string(&self, source_name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .find_map(|scope| scope.get(source_name).map(|info| info.string))
            .unwrap_or(false)
    }

    pub(super) fn binding_is_template(&self, source_name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .find_map(|scope| scope.get(source_name).map(|info| info.template))
            .unwrap_or(false)
    }

    pub(super) fn binding_is_array(&self, source_name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .find_map(|scope| scope.get(source_name).map(|info| info.array))
            .unwrap_or(false)
    }

    pub(super) fn binding_is_map(&self, source_name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .find_map(|scope| scope.get(source_name).map(|info| info.map))
            .unwrap_or(false)
    }

    pub(super) fn binding_is_mutable_collection(&self, source_name: &str) -> bool {
        self.binding_is_array(source_name) || self.binding_is_map(source_name)
    }

    pub(super) fn binding_is_bytes(&self, source_name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .find_map(|scope| scope.get(source_name).map(|info| info.bytes))
            .unwrap_or(false)
    }

    pub(super) fn binding_is_byte_buffer(&self, source_name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .find_map(|scope| scope.get(source_name).map(|info| info.byte_buffer))
            .unwrap_or(false)
    }

    pub(super) fn binding_is_json(&self, source_name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .find_map(|scope| scope.get(source_name).map(|info| info.json))
            .unwrap_or(false)
    }

    pub(super) fn binding_is_option(&self, source_name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .find_map(|scope| scope.get(source_name).map(|info| info.option))
            .unwrap_or(false)
    }

    pub(super) fn binding_is_result(&self, source_name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .find_map(|scope| scope.get(source_name).map(|info| info.result))
            .unwrap_or(false)
    }

    pub(super) fn push_scope(&mut self) {
        let scope_id = self.scope_counter;
        self.scope_counter += 1;
        self.bindings
            .push(Rc::new(std::collections::BTreeMap::new()));
        self.binding_scope_ids.push(scope_id);
    }

    pub(super) fn pop_scope(&mut self) {
        self.bindings.pop();
        self.binding_scope_ids.pop();
        if self.bindings.is_empty() {
            self.bindings
                .push(Rc::new(std::collections::BTreeMap::new()));
            self.binding_scope_ids.push(0);
        }
    }

    pub(super) fn push_loop_frame(&mut self, kind: LoopFrameKind) {
        self.loop_frames.push(kind);
    }

    pub(super) fn pop_loop_frame(&mut self) {
        self.loop_frames.pop();
    }

    pub(super) fn innermost_loop_frame(&self) -> Option<&LoopFrameKind> {
        self.loop_frames.last()
    }

    pub(super) fn function_py_name(&self, source_name: &str) -> Option<&str> {
        self.functions
            .get(source_name)
            .map(|info| info.py_name.as_str())
    }

    pub(super) fn function_info(&self, source_name: &str) -> Option<&FunctionInfo> {
        self.functions.get(source_name)
    }

    pub(super) fn namespace_export(
        &self,
        namespace: &str,
        member: &str,
    ) -> Option<&ModuleRuntimeExport<'a>> {
        let namespace_binding = self.binding_lookup(namespace)?;
        if !namespace_binding.1.namespace_import {
            return None;
        }
        self.namespaces
            .get(namespace)
            .and_then(|exports| exports.get(member))
    }

    pub(super) fn fresh_temp(&mut self, stem: &str) -> String {
        let counter = self.temp_counter;
        self.temp_counter += 1;
        format!("__tpz_{stem}_{counter}")
    }

    pub(super) fn push_pipe_placeholder(&self, replacement: &str) {
        self.pipe_placeholders
            .borrow_mut()
            .push(Rc::from(replacement));
    }

    pub(super) fn pop_pipe_placeholder(&self) {
        self.pipe_placeholders.borrow_mut().pop();
    }

    pub(super) fn pipe_placeholder_replacement(&self) -> Option<String> {
        self.pipe_placeholders
            .borrow()
            .last()
            .map(|replacement| replacement.to_string())
    }

    pub(super) fn with_cooperative_yields<R>(
        &mut self,
        enabled: bool,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = self.cooperative_yields;
        self.cooperative_yields = enabled;
        let result = f(self);
        self.cooperative_yields = previous;
        result
    }
}
