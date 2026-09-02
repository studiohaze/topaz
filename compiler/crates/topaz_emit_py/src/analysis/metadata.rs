use crate::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FunctionInfo {
    pub(super) py_name: String,
    pub(super) cooperative_py_name: Option<String>,
    pub(super) params: Vec<FunctionParamInfo>,
    pub(super) return_shape: Option<ReceiverShape>,
    pub(super) return_wrapped_metadata: WrappedValueMetadataCatalog,
    pub(super) return_record_descendants: RecordDescendantCatalog,
    pub(super) mutated_collection_params: BTreeSet<usize>,
    pub(super) needs_host: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReceiverShape {
    String,
    Template,
    Array,
    Map,
    Bytes,
    ByteBuffer,
    Json,
    Option,
    Result,
}

pub(super) fn is_mutable_collection_shape(shape: Option<ReceiverShape>) -> bool {
    matches!(shape, Some(ReceiverShape::Array | ReceiverShape::Map))
}

pub(super) fn expr_creates_fresh_collection_storage(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Array(_) | ExprKind::MapLiteral(_) => true,
        ExprKind::Comprehension { kind, .. } => {
            matches!(kind, CompKind::Array | CompKind::Map)
        }
        ExprKind::Paren(inner) => expr_creates_fresh_collection_storage(inner),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DirectTailMetadata {
    pub(super) return_shape: Option<ReceiverShape>,
    pub(super) result_ok_shape: Option<ReceiverShape>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FunctionParamInfo {
    pub(super) source_name: String,
    pub(super) py_name: String,
    pub(super) has_default: bool,
    pub(super) variadic: bool,
    pub(super) accepts_named_argument: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum RecordWrapper {
    Option,
    ResultOk,
    MapValue,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WrappedValueMetadata {
    pub(super) receiver_shape: Option<ReceiverShape>,
    pub(super) callable_params: Option<Vec<FunctionParamInfo>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WrappedValueMetadataCatalog {
    pub(super) entries: BTreeMap<Vec<RecordWrapper>, WrappedValueMetadata>,
}

impl WrappedValueMetadataCatalog {
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn root(&self, wrapper: RecordWrapper) -> WrappedValueMetadata {
        self.entries
            .get([wrapper].as_slice())
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn insert_root(&mut self, wrapper: RecordWrapper, metadata: WrappedValueMetadata) {
        if metadata != WrappedValueMetadata::default() {
            self.entries.insert(vec![wrapper], metadata);
        }
    }

    pub(super) fn prepended(mut self, wrapper: RecordWrapper) -> Self {
        self.entries = self
            .entries
            .into_iter()
            .map(|(mut path, metadata)| {
                path.insert(0, wrapper);
                (path, metadata)
            })
            .collect();
        self
    }

    pub(super) fn projected(&self, wrapper: RecordWrapper) -> Self {
        Self {
            entries: self
                .entries
                .iter()
                .filter(|(path, _)| path.first() == Some(&wrapper) && path.len() > 1)
                .map(|(path, metadata)| (path[1..].to_vec(), metadata.clone()))
                .collect(),
        }
    }

    pub(super) fn overlay(&mut self, other: Self) {
        self.entries.extend(other.entries);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RecordDescendantMetadata {
    pub(super) receiver_shapes: BTreeMap<String, ReceiverShape>,
    pub(super) callable_params: BTreeMap<String, Vec<FunctionParamInfo>>,
    pub(super) cooperative_callback_targets: BTreeMap<String, (String, bool)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RecordFieldProjection {
    pub(super) receiver_shape: Option<ReceiverShape>,
    pub(super) callable_params: Option<Vec<FunctionParamInfo>>,
    pub(super) cooperative_callback_target: Option<(String, bool)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecordDescendantReplacement {
    AllAxes,
    PreserveReceiverShapes,
}

impl RecordFieldProjection {
    pub(super) fn overlay(&mut self, other: Self) {
        if other.receiver_shape.is_some() {
            self.receiver_shape = other.receiver_shape;
        }
        if other.callable_params.is_some() {
            self.callable_params = other.callable_params;
        }
        if other.cooperative_callback_target.is_some() {
            self.cooperative_callback_target = other.cooperative_callback_target;
        }
    }
}

impl RecordDescendantMetadata {
    pub(super) fn is_empty(&self) -> bool {
        self.receiver_shapes.is_empty()
            && self.callable_params.is_empty()
            && self.cooperative_callback_targets.is_empty()
    }

    pub(super) fn extend_from(&mut self, other: &Self) {
        self.receiver_shapes.extend(other.receiver_shapes.clone());
        self.callable_params.extend(other.callable_params.clone());
        self.cooperative_callback_targets
            .extend(other.cooperative_callback_targets.clone());
    }

    pub(super) fn field_projection(&self, field_path: &str) -> RecordFieldProjection {
        RecordFieldProjection {
            receiver_shape: self.receiver_shapes.get(field_path).copied(),
            callable_params: self.callable_params.get(field_path).cloned(),
            cooperative_callback_target: self.cooperative_callback_targets.get(field_path).cloned(),
        }
    }

    pub(super) fn replace_field_subtree(
        &mut self,
        field_path: &str,
        replacement: Self,
        policy: RecordDescendantReplacement,
    ) {
        let descendant_prefix = format!("{field_path}.");
        let outside_subtree =
            |path: &str| path != field_path && !path.starts_with(descendant_prefix.as_str());
        if policy == RecordDescendantReplacement::AllAxes {
            self.receiver_shapes.retain(|path, _| outside_subtree(path));
        }
        self.callable_params.retain(|path, _| outside_subtree(path));
        self.cooperative_callback_targets
            .retain(|path, _| outside_subtree(path));
        self.receiver_shapes.extend(replacement.receiver_shapes);
        self.callable_params.extend(replacement.callable_params);
        self.cooperative_callback_targets
            .extend(replacement.cooperative_callback_targets);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RecordDescendantCatalog(
    pub(super) BTreeMap<Vec<RecordWrapper>, RecordDescendantMetadata>,
);

impl RecordDescendantCatalog {
    pub(super) fn metadata(&self, path: &[RecordWrapper]) -> Option<&RecordDescendantMetadata> {
        self.0.get(path)
    }

    pub(super) fn cloned_metadata(&self, path: &[RecordWrapper]) -> RecordDescendantMetadata {
        self.metadata(path).cloned().unwrap_or_default()
    }

    pub(super) fn insert(&mut self, path: Vec<RecordWrapper>, metadata: RecordDescendantMetadata) {
        if !metadata.is_empty() {
            self.0.insert(path, metadata);
        }
    }

    pub(super) fn extend(&mut self, other: Self) {
        for (path, metadata) in other.0 {
            let current = self.0.entry(path).or_default();
            current.receiver_shapes.extend(metadata.receiver_shapes);
            current.callable_params.extend(metadata.callable_params);
            current
                .cooperative_callback_targets
                .extend(metadata.cooperative_callback_targets);
        }
    }

    pub(super) fn prepended(mut self, wrapper: RecordWrapper) -> Self {
        self.0 = self
            .0
            .into_iter()
            .map(|(mut path, metadata)| {
                path.insert(0, wrapper);
                (path, metadata)
            })
            .collect();
        self
    }

    pub(super) fn project(&self, wrapper: RecordWrapper) -> Self {
        Self(
            self.0
                .iter()
                .filter(|(path, _)| path.first() == Some(&wrapper))
                .map(|(path, metadata)| (path[1..].to_vec(), metadata.clone()))
                .collect(),
        )
    }

    pub(super) fn direct_fields_only(mut self) -> Self {
        for metadata in self.0.values_mut() {
            metadata
                .receiver_shapes
                .retain(|field_path, _| !field_path.contains('.'));
            metadata
                .callable_params
                .retain(|field_path, _| !field_path.contains('.'));
            metadata
                .cooperative_callback_targets
                .retain(|field_path, _| !field_path.contains('.'));
        }
        self.0.retain(|_, metadata| !metadata.is_empty());
        self
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

pub(super) fn join_record_descendant_metadata(
    metadata: impl IntoIterator<Item = RecordDescendantMetadata>,
) -> RecordDescendantMetadata {
    let mut metadata = metadata.into_iter();
    let Some(first) = metadata.next() else {
        return RecordDescendantMetadata::default();
    };
    let mut receiver_shapes = Some(first.receiver_shapes);
    let mut callable_params = Some(first.callable_params);
    let mut cooperative_callback_targets = Some(first.cooperative_callback_targets);
    for next in metadata {
        if receiver_shapes.as_ref() != Some(&next.receiver_shapes) {
            receiver_shapes = None;
        }
        if callable_params.as_ref() != Some(&next.callable_params) {
            callable_params = None;
        }
        if cooperative_callback_targets.as_ref() != Some(&next.cooperative_callback_targets) {
            cooperative_callback_targets = None;
        }
    }
    RecordDescendantMetadata {
        receiver_shapes: receiver_shapes.unwrap_or_default(),
        callable_params: callable_params.unwrap_or_default(),
        cooperative_callback_targets: cooperative_callback_targets.unwrap_or_default(),
    }
}

pub(super) fn join_optional_record_descendant_metadata(
    metadata: impl IntoIterator<Item = Option<RecordDescendantMetadata>>,
) -> RecordDescendantMetadata {
    let mut present = Vec::new();
    for metadata in metadata {
        let Some(metadata) = metadata else {
            return RecordDescendantMetadata::default();
        };
        present.push(metadata);
    }
    join_record_descendant_metadata(present)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct StaticMapValueMetadata {
    pub(super) receiver_shape: Option<ReceiverShape>,
    pub(super) callable_params: Option<Vec<FunctionParamInfo>>,
    pub(super) wrapped_value_metadata: WrappedValueMetadataCatalog,
}

impl StaticMapValueMetadata {
    pub(super) fn is_empty(&self) -> bool {
        self.receiver_shape.is_none()
            && self.callable_params.is_none()
            && self.wrapped_value_metadata.is_empty()
    }
}

pub(super) fn homogeneous_map_value_metadata(
    observed: &BTreeMap<String, StaticMapValueMetadata>,
) -> Option<StaticMapValueMetadata> {
    let mut values = observed.values();
    let first = values.next()?;
    values
        .all(|metadata| metadata == first)
        .then(|| first.clone())
}

pub(super) fn static_map_value_metadata_for_value(
    value: &Expr,
    ctx: &Ctx<'_>,
) -> StaticMapValueMetadata {
    StaticMapValueMetadata {
        receiver_shape: receiver_shape_from_value(value, ctx),
        callable_params: callable_param_info(value, ctx),
        wrapped_value_metadata: ctx.wrapped_value_metadata_catalog_for_value(value),
    }
}

pub(super) fn static_map_value_metadata_for_callable_return(
    callback: &Expr,
    ctx: &Ctx<'_>,
) -> Option<StaticMapValueMetadata> {
    let metadata = match &callback.kind {
        ExprKind::Lambda { params, body } => {
            let mut lambda_ctx = ctx.clone();
            lambda_ctx.push_scope();
            for (index, param) in params.iter().enumerate() {
                let source_name = lambda_ctx.text(param.name.span).to_string();
                register_lambda_parameter_binding(
                    &source_name,
                    param,
                    index,
                    None,
                    &mut lambda_ctx,
                );
            }
            static_map_value_metadata_for_value(body, &lambda_ctx)
        }
        ExprKind::Paren(inner) => {
            return static_map_value_metadata_for_callable_return(inner, ctx);
        }
        _ => {
            let info = ctx.function_info_for_call_callee(callback)?;
            StaticMapValueMetadata {
                receiver_shape: info.return_shape,
                callable_params: None,
                wrapped_value_metadata: info.return_wrapped_metadata.clone(),
            }
        }
    };
    (!metadata.is_empty()).then_some(metadata)
}

pub(super) fn observed_map_value_metadata(
    metadata: &MapValueMetadata,
    static_key: Option<&str>,
) -> Option<StaticMapValueMetadata> {
    static_key
        .and_then(|key| metadata.observed_by_key.get(key).cloned())
        .or_else(|| {
            (static_key.is_none() && metadata.observed_keys_complete)
                .then(|| homogeneous_map_value_metadata(&metadata.observed_by_key))
                .flatten()
        })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct MapValueMetadata {
    pub(super) receiver_shape: Option<ReceiverShape>,
    pub(super) wrapped_value_metadata: WrappedValueMetadataCatalog,
    pub(super) declared_callable_params: Option<Vec<FunctionParamInfo>>,
    pub(super) observed_by_key: BTreeMap<String, StaticMapValueMetadata>,
    pub(super) known_present_keys: BTreeSet<String>,
    pub(super) observed_keys_complete: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct StaticArrayElementMetadata {
    pub(super) receiver_shape: Option<ReceiverShape>,
    pub(super) wrapped_value_metadata: WrappedValueMetadataCatalog,
    pub(super) cooperative_callback_target: Option<(String, bool)>,
    pub(super) callable_params: Option<Vec<FunctionParamInfo>>,
    pub(super) map_value: MapValueMetadata,
    pub(super) record_descendants: RecordDescendantCatalog,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ArrayObservationMutation {
    Clear,
    Push(StaticArrayElementMetadata),
    Insert {
        index: usize,
        metadata: StaticArrayElementMetadata,
    },
    Pop,
    RemoveAt(usize),
    Reverse,
    Reorder,
    Retain,
}

pub(super) fn static_array_element_metadata_for_value(
    value: &Expr,
    ctx: &Ctx<'_>,
) -> StaticArrayElementMetadata {
    StaticArrayElementMetadata {
        receiver_shape: receiver_shape_from_value(value, ctx),
        wrapped_value_metadata: ctx.wrapped_value_metadata_catalog_for_value(value),
        cooperative_callback_target: ctx.cooperative_callback_target_for_value(value, false),
        callable_params: (!ctx.namespace_member_value_metadata_origin_for_value(value))
            .then(|| callable_param_info(value, ctx))
            .flatten(),
        map_value: ctx.map_value_metadata_for_value(value, false),
        record_descendants: ctx.record_descendant_catalog_for_value(value, false),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ArrayElementMetadata {
    pub(super) receiver_shape: Option<ReceiverShape>,
    pub(super) declared_callable_params: Option<Vec<FunctionParamInfo>>,
    pub(super) declared_wrapped_value_metadata: WrappedValueMetadataCatalog,
    pub(super) declared_map_value: MapValueMetadata,
    pub(super) declared_record_descendants: RecordDescendantCatalog,
    pub(super) receiver_shapes_by_index: Vec<Option<ReceiverShape>>,
    pub(super) wrapped_value_metadata_by_index: Vec<WrappedValueMetadataCatalog>,
    pub(super) cooperative_callback_targets: Vec<Option<(String, bool)>>,
    pub(super) callable_params_by_index: Vec<Option<Vec<FunctionParamInfo>>>,
    pub(super) map_values_by_index: Vec<MapValueMetadata>,
    pub(super) record_descendants_by_index: Vec<RecordDescendantCatalog>,
    pub(super) static_len: Option<usize>,
}

impl ArrayElementMetadata {
    pub(super) fn is_empty(&self) -> bool {
        self.receiver_shape.is_none()
            && self.declared_callable_params.is_none()
            && self.declared_wrapped_value_metadata.is_empty()
            && self.declared_map_value.is_empty()
            && self.declared_record_descendants.is_empty()
            && self.receiver_shapes_by_index.is_empty()
            && self.wrapped_value_metadata_by_index.is_empty()
            && self.cooperative_callback_targets.is_empty()
            && self.callable_params_by_index.is_empty()
            && self.map_values_by_index.is_empty()
            && self.record_descendants_by_index.is_empty()
            && self.static_len.is_none()
    }

    pub(super) fn replace_observations(&mut self, observations: ArrayElementObservations) {
        self.receiver_shapes_by_index = observations.receiver_shapes_by_index.unwrap_or_default();
        self.wrapped_value_metadata_by_index = observations
            .wrapped_value_metadata_by_index
            .unwrap_or_default();
        self.cooperative_callback_targets = observations
            .cooperative_callback_targets
            .unwrap_or_default();
        self.callable_params_by_index = observations.callable_params_by_index.unwrap_or_default();
        self.map_values_by_index = observations.map_values_by_index.unwrap_or_default();
        self.record_descendants_by_index =
            observations.record_descendants_by_index.unwrap_or_default();
        self.static_len = observations.static_len;
    }

    pub(super) fn clear_observations(&mut self) {
        self.receiver_shapes_by_index.clear();
        self.wrapped_value_metadata_by_index.clear();
        self.cooperative_callback_targets.clear();
        self.callable_params_by_index.clear();
        self.map_values_by_index.clear();
        self.record_descendants_by_index.clear();
        self.static_len = None;
    }

    pub(super) fn set_exact_empty_observations(&mut self) {
        self.clear_observations();
        self.static_len = Some(0);
    }

    pub(super) fn mutate_complete_axis<T>(
        axis: &mut Vec<T>,
        old_len: usize,
        mutation: impl FnOnce(&mut Vec<T>),
    ) {
        if axis.len() == old_len {
            mutation(axis);
        } else {
            axis.clear();
        }
    }

    pub(super) fn push_observation(&mut self, metadata: StaticArrayElementMetadata) {
        let Some(old_len) = self.static_len else {
            self.clear_observations();
            return;
        };
        let StaticArrayElementMetadata {
            receiver_shape,
            wrapped_value_metadata,
            cooperative_callback_target,
            callable_params,
            map_value,
            record_descendants,
        } = metadata;
        Self::mutate_complete_axis(&mut self.receiver_shapes_by_index, old_len, |axis| {
            axis.push(receiver_shape)
        });
        Self::mutate_complete_axis(&mut self.wrapped_value_metadata_by_index, old_len, |axis| {
            axis.push(wrapped_value_metadata)
        });
        Self::mutate_complete_axis(&mut self.cooperative_callback_targets, old_len, |axis| {
            axis.push(cooperative_callback_target)
        });
        Self::mutate_complete_axis(&mut self.callable_params_by_index, old_len, |axis| {
            axis.push(callable_params)
        });
        Self::mutate_complete_axis(&mut self.map_values_by_index, old_len, |axis| {
            axis.push(map_value)
        });
        Self::mutate_complete_axis(&mut self.record_descendants_by_index, old_len, |axis| {
            axis.push(record_descendants)
        });
        self.static_len = old_len.checked_add(1);
    }

    pub(super) fn insert_observation(
        &mut self,
        index: usize,
        metadata: StaticArrayElementMetadata,
    ) {
        let Some(old_len) = self.static_len else {
            self.clear_observations();
            return;
        };
        if index > old_len {
            return;
        }
        let StaticArrayElementMetadata {
            receiver_shape,
            wrapped_value_metadata,
            cooperative_callback_target,
            callable_params,
            map_value,
            record_descendants,
        } = metadata;
        Self::mutate_complete_axis(&mut self.receiver_shapes_by_index, old_len, |axis| {
            axis.insert(index, receiver_shape)
        });
        Self::mutate_complete_axis(&mut self.wrapped_value_metadata_by_index, old_len, |axis| {
            axis.insert(index, wrapped_value_metadata)
        });
        Self::mutate_complete_axis(&mut self.cooperative_callback_targets, old_len, |axis| {
            axis.insert(index, cooperative_callback_target)
        });
        Self::mutate_complete_axis(&mut self.callable_params_by_index, old_len, |axis| {
            axis.insert(index, callable_params)
        });
        Self::mutate_complete_axis(&mut self.map_values_by_index, old_len, |axis| {
            axis.insert(index, map_value)
        });
        Self::mutate_complete_axis(&mut self.record_descendants_by_index, old_len, |axis| {
            axis.insert(index, record_descendants)
        });
        self.static_len = old_len.checked_add(1);
    }

    pub(super) fn remove_observation(&mut self, index: usize) {
        let Some(old_len) = self.static_len else {
            self.clear_observations();
            return;
        };
        if index >= old_len {
            return;
        }
        Self::mutate_complete_axis(&mut self.receiver_shapes_by_index, old_len, |axis| {
            axis.remove(index);
        });
        Self::mutate_complete_axis(&mut self.wrapped_value_metadata_by_index, old_len, |axis| {
            axis.remove(index);
        });
        Self::mutate_complete_axis(&mut self.cooperative_callback_targets, old_len, |axis| {
            axis.remove(index);
        });
        Self::mutate_complete_axis(&mut self.callable_params_by_index, old_len, |axis| {
            axis.remove(index);
        });
        Self::mutate_complete_axis(&mut self.map_values_by_index, old_len, |axis| {
            axis.remove(index);
        });
        Self::mutate_complete_axis(&mut self.record_descendants_by_index, old_len, |axis| {
            axis.remove(index);
        });
        self.static_len = Some(old_len - 1);
    }

    pub(super) fn pop_observation(&mut self) {
        let Some(old_len) = self.static_len else {
            self.clear_observations();
            return;
        };
        if old_len > 0 {
            self.remove_observation(old_len - 1);
        }
    }

    pub(super) fn reverse_observations(&mut self) {
        let Some(old_len) = self.static_len else {
            self.clear_observations();
            return;
        };
        Self::mutate_complete_axis(&mut self.receiver_shapes_by_index, old_len, |axis| {
            axis.reverse()
        });
        Self::mutate_complete_axis(&mut self.wrapped_value_metadata_by_index, old_len, |axis| {
            axis.reverse()
        });
        Self::mutate_complete_axis(&mut self.cooperative_callback_targets, old_len, |axis| {
            axis.reverse()
        });
        Self::mutate_complete_axis(&mut self.callable_params_by_index, old_len, |axis| {
            axis.reverse()
        });
        Self::mutate_complete_axis(&mut self.map_values_by_index, old_len, |axis| {
            axis.reverse()
        });
        Self::mutate_complete_axis(&mut self.record_descendants_by_index, old_len, |axis| {
            axis.reverse()
        });
    }

    pub(super) fn retain_complete_homogeneous_axis<T: PartialEq>(
        axis: &mut Vec<T>,
        old_len: usize,
    ) {
        if axis.len() != old_len
            || axis
                .first()
                .is_some_and(|first| axis.iter().any(|value| value != first))
        {
            axis.clear();
        }
    }

    pub(super) fn retain_complete_homogeneous_callable_axis(
        axis: &mut Vec<Option<Vec<FunctionParamInfo>>>,
        old_len: usize,
    ) {
        if axis.len() != old_len || homogeneous_array_callable_params(axis).is_none() {
            axis.clear();
        }
    }

    pub(super) fn retain_homogeneous_observations(&mut self, exact_length_preserved: bool) {
        let Some(old_len) = self.static_len else {
            self.clear_observations();
            return;
        };
        Self::retain_complete_homogeneous_axis(&mut self.receiver_shapes_by_index, old_len);
        Self::retain_complete_homogeneous_axis(&mut self.wrapped_value_metadata_by_index, old_len);
        Self::retain_complete_homogeneous_axis(&mut self.cooperative_callback_targets, old_len);
        Self::retain_complete_homogeneous_callable_axis(
            &mut self.callable_params_by_index,
            old_len,
        );
        Self::retain_complete_homogeneous_axis(&mut self.map_values_by_index, old_len);
        Self::retain_complete_homogeneous_axis(&mut self.record_descendants_by_index, old_len);
        if !exact_length_preserved && old_len != 0 {
            self.static_len = None;
        }
    }

    pub(super) fn apply_observation_mutation(&mut self, mutation: ArrayObservationMutation) {
        match mutation {
            ArrayObservationMutation::Clear => self.set_exact_empty_observations(),
            ArrayObservationMutation::Push(metadata) => self.push_observation(metadata),
            ArrayObservationMutation::Insert { index, metadata } => {
                self.insert_observation(index, metadata)
            }
            ArrayObservationMutation::Pop => self.pop_observation(),
            ArrayObservationMutation::RemoveAt(index) => self.remove_observation(index),
            ArrayObservationMutation::Reverse => self.reverse_observations(),
            ArrayObservationMutation::Reorder => self.retain_homogeneous_observations(true),
            ArrayObservationMutation::Retain => self.retain_homogeneous_observations(false),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct ArrayElementObservations {
    pub(super) receiver_shapes_by_index: Option<Vec<Option<ReceiverShape>>>,
    pub(super) wrapped_value_metadata_by_index: Option<Vec<WrappedValueMetadataCatalog>>,
    pub(super) cooperative_callback_targets: Option<Vec<Option<(String, bool)>>>,
    pub(super) callable_params_by_index: Option<Vec<Option<Vec<FunctionParamInfo>>>>,
    pub(super) map_values_by_index: Option<Vec<MapValueMetadata>>,
    pub(super) record_descendants_by_index: Option<Vec<RecordDescendantCatalog>>,
    pub(super) static_len: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ArrayElementObservationPolicy {
    pub(super) cooperative_metadata_mutable: bool,
    pub(super) storage_mutable: bool,
}

impl ArrayElementObservationPolicy {
    pub(super) fn binding_registration(metadata_lookup_mutable: bool) -> Self {
        Self {
            cooperative_metadata_mutable: metadata_lookup_mutable,
            storage_mutable: metadata_lookup_mutable,
        }
    }

    pub(super) fn assignment_refresh(storage_mutable: bool) -> Self {
        Self {
            cooperative_metadata_mutable: false,
            storage_mutable,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ArrayElementProjection<'a> {
    pub(super) metadata: &'a ArrayElementMetadata,
    pub(super) static_index: Option<usize>,
    pub(super) observed_slot_metadata: bool,
    pub(super) observed_homogeneous_metadata: bool,
}

impl<'a> ArrayElementProjection<'a> {
    pub(super) fn receiver_shape(self) -> Option<ReceiverShape> {
        self.metadata
            .receiver_shape
            .or_else(|| match self.static_index {
                Some(index) if self.observed_slot_metadata => self
                    .metadata
                    .receiver_shapes_by_index
                    .get(index)
                    .copied()
                    .flatten(),
                None if self.observed_homogeneous_metadata => {
                    homogeneous_array_shape(&self.metadata.receiver_shapes_by_index)
                }
                _ => None,
            })
    }

    pub(super) fn wrapped_value_metadata_catalog(self) -> WrappedValueMetadataCatalog {
        let mut catalog = self.metadata.declared_wrapped_value_metadata.clone();
        let observed = match self.static_index {
            Some(index) if self.observed_slot_metadata => self
                .metadata
                .wrapped_value_metadata_by_index
                .get(index)
                .cloned(),
            None if self.observed_homogeneous_metadata => {
                homogeneous_array_metadata(&self.metadata.wrapped_value_metadata_by_index)
            }
            _ => None,
        };
        if let Some(observed) = observed {
            catalog.overlay(observed);
        }
        catalog
    }

    pub(super) fn callable_params(self) -> Option<Vec<FunctionParamInfo>> {
        if let Some(params) = &self.metadata.declared_callable_params {
            return Some(params.clone());
        }
        match self.static_index {
            Some(index) if self.observed_slot_metadata => self
                .metadata
                .callable_params_by_index
                .get(index)
                .and_then(Clone::clone),
            None if self.observed_homogeneous_metadata => {
                homogeneous_array_callable_params(&self.metadata.callable_params_by_index)
            }
            _ => None,
        }
    }

    pub(super) fn cooperative_callback_target(self) -> Option<(String, bool)> {
        if !self.observed_slot_metadata {
            return None;
        }
        let index = self.static_index?;
        self.metadata
            .cooperative_callback_targets
            .get(index)
            .and_then(Clone::clone)
    }

    pub(super) fn record_field_projection(self, field_path: &str) -> RecordFieldProjection {
        let mut projection = self
            .metadata
            .declared_record_descendants
            .metadata(&[])
            .map(|metadata| metadata.field_projection(field_path))
            .unwrap_or_default();
        if self.observed_slot_metadata
            && let Some(index) = self.static_index
            && let Some(observed) = self
                .metadata
                .record_descendants_by_index
                .get(index)
                .and_then(|catalog| catalog.metadata(&[]))
        {
            projection.overlay(observed.field_projection(field_path));
        }
        projection
    }

    pub(super) fn record_descendant_catalog_under(
        self,
        wrapper: RecordWrapper,
    ) -> RecordDescendantCatalog {
        let mut catalog = self.metadata.declared_record_descendants.project(wrapper);
        if self.observed_slot_metadata
            && let Some(index) = self.static_index
            && let Some(observed) = self.metadata.record_descendants_by_index.get(index)
        {
            catalog.extend(observed.project(wrapper));
        }
        catalog
    }

    pub(super) fn record_descendant_catalog(self) -> RecordDescendantCatalog {
        let mut catalog = self.metadata.declared_record_descendants.clone();
        if self.observed_slot_metadata
            && let Some(index) = self.static_index
            && let Some(observed) = self.metadata.record_descendants_by_index.get(index)
        {
            catalog.extend(observed.clone());
        }
        catalog
    }

    pub(super) fn map_value_pattern_projection(
        self,
        static_key: Option<&str>,
    ) -> MapValuePatternProjection<'a> {
        let observed = if self.observed_slot_metadata {
            self.static_index
                .and_then(|index| self.metadata.map_values_by_index.get(index))
                .and_then(|metadata| observed_map_value_metadata(metadata, static_key))
        } else {
            None
        };
        MapValuePatternProjection {
            metadata: &self.metadata.declared_map_value,
            observed,
            record_descendants: self.record_descendant_catalog_under(RecordWrapper::MapValue),
        }
    }
}

pub(super) fn known_array_slot_observations<T: Clone>(
    slots: &[T],
    static_len: Option<usize>,
) -> Option<Vec<T>> {
    (!slots.is_empty() || static_len == Some(0)).then(|| slots.to_vec())
}

pub(super) fn homogeneous_array_shape(slots: &[Option<ReceiverShape>]) -> Option<ReceiverShape> {
    let mut slots = slots.iter();
    let first = (*slots.next()?)?;
    slots.all(|shape| *shape == Some(first)).then_some(first)
}

pub(super) fn homogeneous_array_metadata<T: Clone + PartialEq>(slots: &[T]) -> Option<T> {
    let mut slots = slots.iter();
    let first = slots.next()?;
    slots
        .all(|metadata| metadata == first)
        .then(|| first.clone())
}

pub(super) fn join_array_element_observations(
    observations: impl IntoIterator<Item = ArrayElementObservations>,
) -> ArrayElementObservations {
    let mut observations = observations.into_iter();
    let Some(first) = observations.next() else {
        return ArrayElementObservations::default();
    };
    let mut receiver_shapes = first.receiver_shapes_by_index;
    let mut wrapped_value_metadata = first.wrapped_value_metadata_by_index;
    let mut targets = first.cooperative_callback_targets;
    let mut params = first.callable_params_by_index;
    let mut map_values = first.map_values_by_index;
    let mut record_descendants = first.record_descendants_by_index;
    let mut static_len = first.static_len;
    for next in observations {
        if receiver_shapes != next.receiver_shapes_by_index {
            receiver_shapes = None;
        }
        if wrapped_value_metadata != next.wrapped_value_metadata_by_index {
            wrapped_value_metadata = None;
        }
        if targets != next.cooperative_callback_targets {
            targets = None;
        }
        if params != next.callable_params_by_index {
            params = None;
        }
        if map_values != next.map_values_by_index {
            map_values = None;
        }
        if record_descendants != next.record_descendants_by_index {
            record_descendants = None;
        }
        if static_len != next.static_len {
            static_len = None;
        }
    }
    ArrayElementObservations {
        receiver_shapes_by_index: receiver_shapes,
        wrapped_value_metadata_by_index: wrapped_value_metadata,
        cooperative_callback_targets: targets,
        callable_params_by_index: params,
        map_values_by_index: map_values,
        record_descendants_by_index: record_descendants,
        static_len,
    }
}

impl MapValueMetadata {
    pub(super) fn is_empty(&self) -> bool {
        self.receiver_shape.is_none()
            && self.wrapped_value_metadata.is_empty()
            && self.declared_callable_params.is_none()
            && self.observed_by_key.is_empty()
            && self.known_present_keys.is_empty()
            && !self.observed_keys_complete
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct ModuleValueMetadata {
    pub(super) callable_params: Option<Vec<FunctionParamInfo>>,
    pub(super) mutated_collection_params: BTreeSet<usize>,
    pub(super) array_elements: ArrayElementMetadata,
    pub(super) record_descendants: RecordDescendantCatalog,
    pub(super) map_value: MapValueMetadata,
    pub(super) receiver_shape: Option<ReceiverShape>,
    pub(super) wrapped_value_metadata: WrappedValueMetadataCatalog,
}

impl ModuleValueMetadata {
    pub(super) fn empty() -> Self {
        Self::default()
    }
}

#[derive(Debug)]
pub(super) enum ModuleRuntimeExport<'a> {
    Value {
        py_name: String,
        cooperative_callback: Option<(String, bool)>,
        metadata: Box<ModuleValueMetadata>,
    },
    Function {
        info: FunctionInfo,
    },
    Record {
        record: NominalRecordDef<'a>,
        receiver_methods: BTreeMap<String, FunctionInfo>,
    },
    Newtype {
        newtype: NewtypeDef,
        receiver_methods: BTreeMap<String, FunctionInfo>,
    },
    Enum {
        enum_def: EnumDef,
        receiver_methods: BTreeMap<String, FunctionInfo>,
    },
}

impl ModuleRuntimeExport<'_> {
    pub(super) fn runtime_py_name(&self) -> Option<&str> {
        match self {
            ModuleRuntimeExport::Value { py_name, .. } => Some(py_name),
            ModuleRuntimeExport::Function { info } => Some(&info.py_name),
            ModuleRuntimeExport::Record { .. } => None,
            ModuleRuntimeExport::Newtype { .. } => None,
            ModuleRuntimeExport::Enum { .. } => None,
        }
    }
}

pub(super) type ModuleRuntimeExportMap<'a> = BTreeMap<String, ModuleRuntimeExport<'a>>;
pub(super) type ModuleRuntimeExports<'a> = Rc<ModuleRuntimeExportMap<'a>>;

impl<'a> Ctx<'a> {
    pub(super) fn clear_collection_alias_value_metadata(&mut self, source_name: &str) {
        self.mutate_collection_alias_value_metadata(source_name, clear_binding_info_value_metadata);
    }

    pub(super) fn mutate_collection_alias_value_metadata(
        &mut self,
        source_name: &str,
        mut mutate: impl FnMut(&mut BindingInfo),
    ) {
        let identity = self
            .binding_lookup(source_name)
            .and_then(|(_, info)| info.collection_storage_identity.clone());
        let Some(identity) = identity else {
            if let Some((_, info)) = self.binding_lookup_mut(source_name) {
                mutate(info);
            }
            return;
        };
        for scope in &mut self.bindings {
            for info in Rc::make_mut(scope).values_mut() {
                if info.collection_storage_identity.as_ref() == Some(&identity) {
                    mutate(info);
                }
            }
        }
    }

    pub(super) fn apply_collection_alias_array_observation_mutation(
        &mut self,
        source_name: &str,
        mutation: ArrayObservationMutation,
    ) {
        self.mutate_collection_alias_value_metadata(source_name, |info| {
            let namespace_origin = collection_storage_has_namespace_metadata_origin(info);
            info.array_elements
                .apply_observation_mutation(mutation.clone());
            set_namespace_member_value_metadata_from_origin(info, namespace_origin);
        });
    }

    pub(super) fn update_collection_alias_map_value_metadata(
        &mut self,
        source_name: &str,
        key: String,
        value: &Expr,
    ) {
        let metadata = static_map_value_metadata_for_value(value, self);
        self.mutate_collection_alias_value_metadata(source_name, |info| {
            let namespace_origin = collection_storage_has_namespace_metadata_origin(info);
            info.map_value
                .observed_by_key
                .insert(key.clone(), metadata.clone());
            info.map_value.known_present_keys.insert(key.clone());
            set_namespace_member_value_metadata_from_origin(info, namespace_origin);
        });
    }

    pub(super) fn update_collection_alias_map_value_metadata_from_update(
        &mut self,
        source_name: &str,
        key: String,
        initial: &Expr,
        callback: &Expr,
    ) {
        let Some((_, current)) = self.binding_lookup(source_name) else {
            return;
        };
        let current_map = current.map_value.clone();
        let initial_metadata = static_map_value_metadata_for_value(initial, self);
        let callback_metadata = static_map_value_metadata_for_callable_return(callback, self);
        let next_metadata = if current_map.known_present_keys.contains(&key) {
            callback_metadata
        } else if (current_map.observed_keys_complete
            && !current_map.observed_by_key.contains_key(&key))
            || callback_metadata.as_ref() == Some(&initial_metadata)
        {
            Some(initial_metadata)
        } else {
            None
        };
        self.mutate_collection_alias_value_metadata(source_name, |info| {
            let namespace_origin = collection_storage_has_namespace_metadata_origin(info);
            if let Some(metadata) = &next_metadata {
                info.map_value
                    .observed_by_key
                    .insert(key.clone(), metadata.clone());
            } else {
                info.map_value.observed_by_key.remove(&key);
                info.map_value.observed_keys_complete = false;
            }
            info.map_value.known_present_keys.insert(key.clone());
            set_namespace_member_value_metadata_from_origin(info, namespace_origin);
        });
    }

    pub(super) fn remove_collection_alias_map_value_metadata(
        &mut self,
        source_name: &str,
        key: &str,
    ) {
        self.mutate_collection_alias_value_metadata(source_name, |info| {
            let namespace_origin = info.namespace_member_value_metadata;
            info.map_value.observed_by_key.remove(key);
            info.map_value.known_present_keys.remove(key);
            set_namespace_member_value_metadata_from_origin(info, namespace_origin);
        });
    }

    pub(super) fn clear_collection_alias_map_known_presence(&mut self, source_name: &str) {
        self.mutate_collection_alias_value_metadata(source_name, |info| {
            info.map_value.known_present_keys.clear();
        });
    }

    pub(super) fn clear_collection_alias_map_value_metadata(&mut self, source_name: &str) {
        self.mutate_collection_alias_value_metadata(source_name, |info| {
            let namespace_origin = collection_storage_has_namespace_metadata_origin(info);
            info.map_value.observed_by_key.clear();
            info.map_value.known_present_keys.clear();
            info.map_value.observed_keys_complete = true;
            set_namespace_member_value_metadata_from_origin(info, namespace_origin);
        });
    }

    pub(super) fn clear_record_value_metadata(&mut self, source_name: &str) {
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.record_descendants = RecordDescendantCatalog::default();
            info.namespace_member_value_metadata = false;
        }
    }

    pub(super) fn clear_map_value_observations(&mut self, source_name: &str) {
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.map_value.observed_by_key.clear();
            info.map_value.known_present_keys.clear();
            info.map_value.observed_keys_complete = false;
            info.namespace_member_value_metadata = false;
        }
    }

    pub(super) fn clear_callable_value_metadata(&mut self, source_name: &str) {
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.cooperative_callback_py_name = None;
            info.cooperative_callback_needs_host = false;
            info.callable_params = None;
            info.mutated_collection_params.clear();
            info.callable_params_flow_allowed = false;
            info.composed = false;
            info.namespace_member_value_metadata = false;
        }
    }

    pub(super) fn refresh_callable_value_metadata_from_value(
        &mut self,
        source_name: &str,
        value: &Expr,
    ) {
        let namespace_member_value_metadata =
            self.namespace_member_value_metadata_origin_for_value(value);
        let flow_callable_params = callable_param_info(value, self);
        let (callable_params, callable_params_flow_allowed) = if flow_callable_params.is_some() {
            (flow_callable_params, true)
        } else {
            (direct_call_callable_param_info(value, self), false)
        };
        let cooperative_callback_target =
            self.cooperative_callback_target_for_value(value, !namespace_member_value_metadata);
        let composed = compose_binding_value(value, self);
        let mutated_collection_params = if callable_params_flow_allowed {
            self.callable_mutated_collection_params_for_value(value)
        } else {
            BTreeSet::new()
        };
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.callable_params = callable_params;
            info.mutated_collection_params = mutated_collection_params;
            info.callable_params_flow_allowed = callable_params_flow_allowed;
            info.cooperative_callback_py_name = cooperative_callback_target
                .as_ref()
                .map(|target| target.0.clone());
            info.cooperative_callback_needs_host = cooperative_callback_target
                .as_ref()
                .is_some_and(|target| target.1);
            info.composed = composed;
            set_namespace_member_value_metadata_from_origin(info, namespace_member_value_metadata);
        }
    }

    pub(super) fn refresh_array_element_observations_from_value(
        &mut self,
        source_name: &str,
        value: &Expr,
    ) {
        let namespace_member_value_metadata =
            self.namespace_member_value_metadata_origin_for_value(value);
        let observations = self.array_element_observations_for_value(
            value,
            ArrayElementObservationPolicy::assignment_refresh(self.binding_is_mutable(source_name)),
        );
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.array_elements.replace_observations(observations);
            set_namespace_member_value_metadata_from_origin(info, namespace_member_value_metadata);
        }
    }

    pub(super) fn update_array_element_observation_from_value(
        &mut self,
        source_name: &str,
        index: usize,
        value: &Expr,
    ) {
        let target = self.cooperative_callback_target_for_value(value, false);
        let receiver_shape = receiver_shape_from_value(value, self);
        let wrapped_value_metadata = self.wrapped_value_metadata_catalog_for_value(value);
        let callable_params = if self.namespace_member_value_metadata_origin_for_value(value) {
            None
        } else {
            callable_param_info(value, self)
        };
        let map_value = self.map_value_metadata_for_value(value, false);
        let record_descendants = self.record_descendant_catalog_for_value(value, false);
        self.mutate_collection_alias_value_metadata(source_name, |info| {
            let namespace_origin = collection_storage_has_namespace_metadata_origin(info);
            let in_range = info.array_elements.static_len.map_or(
                index < info.array_elements.cooperative_callback_targets.len(),
                |len| index < len,
            );
            if in_range {
                if index < info.array_elements.receiver_shapes_by_index.len() {
                    info.array_elements.receiver_shapes_by_index[index] = receiver_shape;
                } else {
                    info.array_elements.receiver_shapes_by_index.clear();
                }
                if index < info.array_elements.wrapped_value_metadata_by_index.len() {
                    info.array_elements.wrapped_value_metadata_by_index[index] =
                        wrapped_value_metadata.clone();
                } else {
                    info.array_elements.wrapped_value_metadata_by_index.clear();
                }
                if index < info.array_elements.cooperative_callback_targets.len() {
                    info.array_elements.cooperative_callback_targets[index] = target.clone();
                } else {
                    info.array_elements.cooperative_callback_targets.clear();
                }
                if index < info.array_elements.callable_params_by_index.len() {
                    info.array_elements.callable_params_by_index[index] = callable_params.clone();
                } else {
                    info.array_elements.callable_params_by_index.clear();
                }
                if index < info.array_elements.map_values_by_index.len() {
                    info.array_elements.map_values_by_index[index] = map_value.clone();
                } else {
                    info.array_elements.map_values_by_index.clear();
                }
                if index < info.array_elements.record_descendants_by_index.len() {
                    info.array_elements.record_descendants_by_index[index] =
                        record_descendants.clone();
                } else {
                    info.array_elements.record_descendants_by_index.clear();
                }
            } else {
                info.array_elements.clear_observations();
            }
            set_namespace_member_value_metadata_from_origin(info, namespace_origin);
        });
    }

    pub(super) fn update_array_element_record_path_metadata_from_value(
        &mut self,
        source_name: &str,
        index: usize,
        field_path: &str,
        value: &Expr,
    ) {
        let mut assigned_metadata = RecordDescendantMetadata::default();
        self.apply_record_field_value_metadata(&mut assigned_metadata, field_path, value, true);
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.namespace_member_value_metadata = false;
            let Some(catalog) = info
                .array_elements
                .record_descendants_by_index
                .get_mut(index)
            else {
                info.array_elements.record_descendants_by_index.clear();
                return;
            };
            let metadata = catalog.0.entry(Vec::new()).or_default();
            metadata.replace_field_subtree(
                field_path,
                assigned_metadata,
                RecordDescendantReplacement::AllAxes,
            );
            if metadata.is_empty() {
                catalog.0.remove(&Vec::new());
            }
        }
    }

    pub(super) fn refresh_record_value_metadata_from_value(
        &mut self,
        source_name: &str,
        value: &Expr,
    ) {
        let namespace_member_value_metadata =
            self.namespace_member_value_metadata_origin_for_value(value);
        let metadata_lookup_mutable =
            self.binding_is_mutable(source_name) && !namespace_member_value_metadata;
        let record_descendants = self
            .record_descendant_catalog_for_value(value, metadata_lookup_mutable)
            .direct_fields_only();
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.record_descendants = record_descendants;
            set_namespace_member_value_metadata_from_origin(info, namespace_member_value_metadata);
        }
    }

    pub(super) fn refresh_map_value_observations_from_value(
        &mut self,
        source_name: &str,
        value: &Expr,
    ) {
        let namespace_member_value_metadata =
            self.namespace_member_value_metadata_origin_for_value(value);
        let observed = if namespace_member_value_metadata {
            self.map_value_metadata_for_value(value, false)
        } else {
            MapValueMetadata::default()
        };
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.map_value.observed_by_key = observed.observed_by_key;
            info.map_value.known_present_keys = observed.known_present_keys;
            info.map_value.observed_keys_complete = observed.observed_keys_complete;
            set_namespace_member_value_metadata_from_origin(info, namespace_member_value_metadata);
        }
    }

    pub(super) fn update_record_path_metadata_from_value(
        &mut self,
        source_name: &str,
        field_path: &str,
        value: &Expr,
    ) {
        let mut assigned_metadata = RecordDescendantMetadata::default();
        self.apply_record_field_value_metadata(&mut assigned_metadata, field_path, value, true);
        if let Some((_, info)) = self.binding_lookup_mut(source_name) {
            info.namespace_member_value_metadata = false;
            info.record_descendants.0.retain(|path, _| path.is_empty());
            let metadata = info.record_descendants.0.entry(Vec::new()).or_default();
            metadata.replace_field_subtree(
                field_path,
                assigned_metadata,
                RecordDescendantReplacement::AllAxes,
            );
            if metadata.is_empty() {
                info.record_descendants.0.remove(&Vec::new());
            }
        }
    }

    pub(super) fn binding_py_name(&self, source_name: &str) -> Option<&str> {
        self.binding_lookup(source_name)
            .and_then(|(_, info)| info.py_name.as_deref())
            .or_else(|| {
                self.module_value_py_names
                    .get(source_name)
                    .map(String::as_str)
            })
    }

    pub(super) fn binding_is_forward_function_cell(&self, source_name: &str) -> bool {
        self.binding_lookup(source_name)
            .is_some_and(|(_, info)| info.forward_function_cell)
    }

    pub(super) fn forward_function_value_py(
        &self,
        source_name: &str,
        cell_py_name: &str,
        span: Span,
    ) -> String {
        format!(
            "__tpz_forward_function({}, {}, {})",
            cell_py_name,
            py_string(source_name),
            py_span(span)
        )
    }

    pub(super) fn binding_cooperative_callback_target(
        &self,
        source_name: &str,
        span: Span,
    ) -> Option<(String, bool)> {
        self.binding_lookup(source_name).and_then(|(_, info)| {
            if !self.binding_allows_value_static_metadata(source_name, info)
                || (info.mutable && !info.namespace_member_value_metadata)
            {
                return None;
            }
            info.cooperative_callback_py_name.as_ref().map(|py_name| {
                let py_name = if info.forward_function_cell {
                    self.forward_function_value_py(source_name, py_name, span)
                } else {
                    py_name.clone()
                };
                (py_name, info.cooperative_callback_needs_host)
            })
        })
    }

    pub(super) fn record_member_path_for_field(
        &self,
        object: &Expr,
        field: &Ident,
    ) -> Option<(String, String)> {
        let mut fields = vec![self.text(field.span).to_string()];
        let source_name = self.record_member_path_root(object, &mut fields)?;
        fields.reverse();
        Some((source_name, fields.join(".")))
    }

    pub(super) fn record_member_path_parts_for_field(
        &self,
        object: &Expr,
        field: &Ident,
    ) -> Option<(String, Vec<String>)> {
        let mut fields = vec![self.text(field.span).to_string()];
        let source_name = self.record_member_path_root(object, &mut fields)?;
        fields.reverse();
        Some((source_name, fields))
    }

    pub(super) fn record_member_path_root(
        &self,
        object: &Expr,
        reversed_fields: &mut Vec<String>,
    ) -> Option<String> {
        match &object.kind {
            ExprKind::Ident => Some(self.text(object.span).to_string()),
            ExprKind::Member { object, field } => {
                reversed_fields.push(self.text(field.span).to_string());
                self.record_member_path_root(object, reversed_fields)
            }
            ExprKind::Paren(inner) => self.record_member_path_root(inner, reversed_fields),
            _ => None,
        }
    }

    pub(super) fn member_path_for_expr(&self, expr: &Expr) -> Option<(String, Vec<String>)> {
        let mut fields = Vec::new();
        let source_name = self.record_member_path_root(expr, &mut fields)?;
        if fields.is_empty() {
            return None;
        }
        fields.reverse();
        Some((source_name, fields))
    }

    pub(super) fn namespace_value_metadata(
        &self,
        namespace: &str,
        member: &str,
    ) -> Option<&ModuleValueMetadata> {
        match self.namespace_export(namespace, member)? {
            ModuleRuntimeExport::Value { metadata, .. } => Some(metadata.as_ref()),
            _ => None,
        }
    }

    pub(super) fn namespace_value_metadata_for_member_expr(
        &self,
        expr: &Expr,
    ) -> Option<&ModuleValueMetadata> {
        let (namespace, fields) = self.member_path_for_expr(expr)?;
        if fields.len() != 1 {
            return None;
        }
        self.namespace_value_metadata(&namespace, &fields[0])
    }

    pub(super) fn namespace_collection_storage_identity_for_member_expr(
        &self,
        expr: &Expr,
    ) -> Option<CollectionStorageIdentity> {
        let (namespace, fields) = self.member_path_for_expr(expr)?;
        if fields.len() != 1 {
            return None;
        }
        match self.namespace_export(&namespace, &fields[0])? {
            ModuleRuntimeExport::Value {
                py_name, metadata, ..
            } if is_mutable_collection_shape(metadata.receiver_shape) => {
                Some(CollectionStorageIdentity::Namespace(py_name.clone()))
            }
            _ => None,
        }
    }

    pub(super) fn namespace_function_info_for_member_expr(
        &self,
        expr: &Expr,
    ) -> Option<&FunctionInfo> {
        let (namespace, fields) = self.member_path_for_expr(expr)?;
        if fields.len() != 1 {
            return None;
        }
        match self.namespace_export(&namespace, &fields[0])? {
            ModuleRuntimeExport::Function { info } => Some(info),
            _ => None,
        }
    }

    pub(super) fn namespace_value_callable_params_for_member_expr(
        &self,
        expr: &Expr,
    ) -> Option<Vec<FunctionParamInfo>> {
        if let Some(info) = self.namespace_function_info_for_member_expr(expr) {
            return Some(info.params.clone());
        }
        self.namespace_value_metadata_for_member_expr(expr)
            .and_then(|metadata| metadata.callable_params.clone())
    }

    pub(super) fn namespace_value_cooperative_callback_target_for_member_expr(
        &self,
        expr: &Expr,
    ) -> Option<(String, bool)> {
        if let Some(info) = self.namespace_function_info_for_member_expr(expr) {
            return info
                .cooperative_py_name
                .as_ref()
                .map(|py_name| (py_name.clone(), info.needs_host));
        }
        let (namespace, fields) = self.member_path_for_expr(expr)?;
        if fields.len() != 1 {
            return None;
        }
        match self.namespace_export(&namespace, &fields[0])? {
            ModuleRuntimeExport::Value {
                cooperative_callback,
                ..
            } => cooperative_callback.clone(),
            _ => None,
        }
    }

    pub(super) fn namespace_member_value_metadata_origin_for_value(&self, value: &Expr) -> bool {
        match &value.kind {
            ExprKind::Member { .. } => {
                self.namespace_value_metadata_for_member_expr(value)
                    .is_some()
                    || self
                        .namespace_function_info_for_member_expr(value)
                        .is_some()
            }
            ExprKind::Ident => {
                let name = self.text(value.span);
                self.binding_lookup(name).is_some_and(|(_, info)| {
                    self.binding_allows_flow_static_metadata(name, info)
                        && info.namespace_member_value_metadata
                })
            }
            ExprKind::Paren(inner) => self.namespace_member_value_metadata_origin_for_value(inner),
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => join_identical_if_branch_metadata(then_block, else_branch.as_deref(), |branch| {
                self.namespace_member_value_metadata_origin_for_value(branch)
            })
            .unwrap_or(false),
            ExprKind::Match { cases, .. } => join_identical_match_arm_metadata(cases, |arm| {
                self.namespace_member_value_metadata_origin_for_value(arm)
            })
            .unwrap_or(false),
            _ => false,
        }
    }

    pub(super) fn collection_storage_identity_for_value(
        &self,
        value: &Expr,
    ) -> Option<CollectionStorageIdentity> {
        match &value.kind {
            ExprKind::Member { .. } => {
                self.namespace_collection_storage_identity_for_member_expr(value)
            }
            ExprKind::Ident => {
                let name = self.text(value.span);
                self.binding_lookup(name)
                    .filter(|(_, info)| self.binding_allows_flow_static_metadata(name, info))
                    .and_then(|(_, info)| info.collection_storage_identity.clone())
            }
            ExprKind::Paren(inner) => self.collection_storage_identity_for_value(inner),
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => join_identical_if_branch_metadata(then_block, else_branch.as_deref(), |branch| {
                self.collection_storage_identity_for_value(branch)
            })
            .flatten(),
            ExprKind::Match { cases, .. } => join_identical_match_arm_metadata(cases, |arm| {
                self.collection_storage_identity_for_value(arm)
            })
            .flatten(),
            _ => None,
        }
    }

    pub(super) fn collection_storage_identity_for_binding_value(
        &self,
        value: &Expr,
        mutable: bool,
    ) -> Option<CollectionStorageIdentity> {
        self.collection_storage_identity_for_value(value)
            .or_else(|| {
                (mutable && expr_creates_fresh_collection_storage(value)).then(|| {
                    CollectionStorageIdentity::Local {
                        module: self.module_identity.to_string(),
                        span: value.span,
                    }
                })
            })
    }

    pub(super) fn namespace_record_metadata_for_field(
        &self,
        object: &Expr,
        field: &Ident,
    ) -> Option<(&ModuleValueMetadata, String)> {
        let (namespace, fields) = self.record_member_path_parts_for_field(object, field)?;
        let (member, field_path) = fields.split_first()?;
        if field_path.is_empty() {
            return None;
        }
        let metadata = self.namespace_value_metadata(&namespace, member)?;
        Some((metadata, field_path.join(".")))
    }

    pub(super) fn record_member_field_projection(
        &self,
        object: &Expr,
        field: &Ident,
    ) -> RecordFieldProjection {
        if let Some((metadata, field_path)) =
            self.namespace_record_metadata_for_field(object, field)
        {
            return metadata
                .record_descendants
                .metadata(&[])
                .map(|metadata| metadata.field_projection(&field_path))
                .unwrap_or_default();
        }
        let mut reversed_fields = vec![self.text(field.span).to_string()];
        if let Some((array, index)) =
            self.array_element_record_member_path_root(object, &mut reversed_fields)
        {
            reversed_fields.reverse();
            return self
                .array_element_projection_for_index(array, index)
                .map(|projection| projection.record_field_projection(&reversed_fields.join(".")))
                .unwrap_or_default();
        }
        let mut reversed_fields = vec![self.text(field.span).to_string()];
        if let Some(info) = self.call_return_record_member_path_root(object, &mut reversed_fields) {
            reversed_fields.reverse();
            return info
                .return_record_descendants
                .metadata(&[])
                .map(|metadata| metadata.field_projection(&reversed_fields.join(".")))
                .unwrap_or_default();
        }
        let Some((source_name, field_path)) = self.record_member_path_for_field(object, field)
        else {
            return RecordFieldProjection::default();
        };
        self.binding_lookup(&source_name)
            .map(|(_, info)| {
                self.binding_record_descendant_field_projection(
                    &source_name,
                    info,
                    &[],
                    &field_path,
                )
            })
            .unwrap_or_default()
    }

    pub(super) fn array_element_record_member_path_root<'b>(
        &self,
        object: &'b Expr,
        reversed_fields: &mut Vec<String>,
    ) -> Option<(&'b Expr, &'b Expr)> {
        match &object.kind {
            ExprKind::Index { object, index } => Some((object, index)),
            ExprKind::Member { object, field } => {
                reversed_fields.push(self.text(field.span).to_string());
                self.array_element_record_member_path_root(object, reversed_fields)
            }
            ExprKind::Paren(inner) => {
                self.array_element_record_member_path_root(inner, reversed_fields)
            }
            _ => None,
        }
    }

    pub(super) fn array_element_projection_for_index<'b>(
        &'b self,
        object: &Expr,
        index: &Expr,
    ) -> Option<ArrayElementProjection<'b>> {
        let static_index = self.static_usize_index(index);
        match &object.kind {
            ExprKind::Ident => {
                let source_name = self.text(object.span);
                let (_, info) = self.binding_lookup(source_name)?;
                let observed_slot_metadata =
                    self.binding_allows_value_static_metadata(source_name, info);
                Some(ArrayElementProjection {
                    metadata: &info.array_elements,
                    static_index,
                    observed_slot_metadata,
                    observed_homogeneous_metadata: (!info.mutable
                        || collection_storage_is_local(info))
                        && observed_slot_metadata,
                })
            }
            ExprKind::Member { .. } => {
                let metadata = self.namespace_value_metadata_for_member_expr(object)?;
                Some(ArrayElementProjection {
                    metadata: &metadata.array_elements,
                    static_index,
                    observed_slot_metadata: true,
                    observed_homogeneous_metadata: true,
                })
            }
            ExprKind::Paren(inner) => self.array_element_projection_for_index(inner, index),
            _ => None,
        }
    }

    pub(super) fn array_element_callable_params_for_index(
        &self,
        object: &Expr,
        index: &Expr,
    ) -> Option<Vec<FunctionParamInfo>> {
        self.array_element_projection_for_index(object, index)
            .and_then(ArrayElementProjection::callable_params)
    }

    pub(super) fn receiver_shape_for_member_expr(&self, expr: &Expr) -> Option<ReceiverShape> {
        if let Some(shape) = self
            .namespace_value_metadata_for_member_expr(expr)
            .and_then(|metadata| metadata.receiver_shape)
        {
            return Some(shape);
        }
        match &expr.kind {
            ExprKind::Member { object, field } => {
                self.record_member_field_projection(object, field)
                    .receiver_shape
            }
            ExprKind::Paren(inner) => self.receiver_shape_for_member_expr(inner),
            _ => None,
        }
    }

    pub(super) fn receiver_shape_for_index_expr(&self, expr: &Expr) -> Option<ReceiverShape> {
        match &expr.kind {
            ExprKind::Index { object, index } => self
                .array_element_projection_for_index(object, index)
                .and_then(ArrayElementProjection::receiver_shape),
            ExprKind::Paren(inner) => self.receiver_shape_for_index_expr(inner),
            _ => None,
        }
    }

    pub(super) fn call_return_record_member_path_root<'b>(
        &'b self,
        object: &Expr,
        reversed_fields: &mut Vec<String>,
    ) -> Option<&'b FunctionInfo> {
        match &object.kind {
            ExprKind::Call { callee, .. } => self.function_info_for_call_callee(callee),
            ExprKind::Member { object, field } => {
                reversed_fields.push(self.text(field.span).to_string());
                self.call_return_record_member_path_root(object, reversed_fields)
            }
            ExprKind::Paren(inner) => {
                self.call_return_record_member_path_root(inner, reversed_fields)
            }
            _ => None,
        }
    }

    pub(super) fn function_info_for_call_callee(&self, callee: &Expr) -> Option<&FunctionInfo> {
        match &callee.kind {
            ExprKind::Ident => {
                let name = self.text(callee.span);
                if self.binding_is_bound(name) {
                    None
                } else {
                    self.function_info(name)
                }
            }
            ExprKind::Member { object, field } => {
                let ExprKind::Ident = &object.kind else {
                    return None;
                };
                let namespace = self.text(object.span);
                match self.namespace_export(namespace, self.text(field.span))? {
                    ModuleRuntimeExport::Function { info } => Some(info),
                    _ => None,
                }
            }
            ExprKind::Paren(inner) => self.function_info_for_call_callee(inner),
            _ => None,
        }
    }

    pub(super) fn static_usize_index(&self, expr: &Expr) -> Option<usize> {
        match &expr.kind {
            ExprKind::Int => self.text(expr.span).replace('_', "").parse::<usize>().ok(),
            ExprKind::Paren(inner) => self.static_usize_index(inner),
            _ => None,
        }
    }

    pub(super) fn spread_array_element_observations_for_value(
        &self,
        value: &Expr,
    ) -> ArrayElementObservations {
        let metadata = match &value.kind {
            ExprKind::Ident => {
                let name = self.text(value.span);
                self.binding_lookup(name)
                    .filter(|(_, info)| !info.mutable || collection_storage_is_local(info))
                    .map(|(_, info)| info.array_elements.clone())
            }
            ExprKind::Member { .. } => self
                .namespace_value_metadata_for_member_expr(value)
                .map(|metadata| metadata.array_elements.clone()),
            ExprKind::Paren(inner) => {
                return self.spread_array_element_observations_for_value(inner);
            }
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => {
                let Some(then_expr) = metadata_join_block_tail_expr(then_block) else {
                    return ArrayElementObservations::default();
                };
                let Some(else_expr) = else_branch.as_deref().and_then(metadata_join_else_expr)
                else {
                    return ArrayElementObservations::default();
                };
                return join_array_element_observations([
                    self.spread_array_element_observations_for_value(then_expr),
                    self.spread_array_element_observations_for_value(else_expr),
                ]);
            }
            ExprKind::Match { cases, .. }
                if !cases.is_empty()
                    && cases.last().is_some_and(match_case_is_unguarded_catch_all) =>
            {
                let observations = cases.iter().map(|case| {
                    metadata_join_match_body_expr(&case.body)
                        .map(|arm| self.spread_array_element_observations_for_value(arm))
                        .unwrap_or_default()
                });
                return join_array_element_observations(observations);
            }
            _ => None,
        };
        let Some(metadata) = metadata else {
            return ArrayElementObservations::default();
        };
        ArrayElementObservations {
            receiver_shapes_by_index: known_array_slot_observations(
                &metadata.receiver_shapes_by_index,
                metadata.static_len,
            ),
            wrapped_value_metadata_by_index: known_array_slot_observations(
                &metadata.wrapped_value_metadata_by_index,
                metadata.static_len,
            ),
            cooperative_callback_targets: known_array_slot_observations(
                &metadata.cooperative_callback_targets,
                metadata.static_len,
            ),
            callable_params_by_index: known_array_slot_observations(
                &metadata.callable_params_by_index,
                metadata.static_len,
            ),
            map_values_by_index: known_array_slot_observations(
                &metadata.map_values_by_index,
                metadata.static_len,
            ),
            record_descendants_by_index: known_array_slot_observations(
                &metadata.record_descendants_by_index,
                metadata.static_len,
            ),
            static_len: metadata.static_len,
        }
    }

    pub(super) fn array_element_observations_for_value(
        &self,
        value: &Expr,
        policy: ArrayElementObservationPolicy,
    ) -> ArrayElementObservations {
        match &value.kind {
            ExprKind::Array(elements) => {
                let mut observations = ArrayElementObservations {
                    receiver_shapes_by_index: Some(Vec::with_capacity(elements.len())),
                    wrapped_value_metadata_by_index: Some(Vec::with_capacity(elements.len())),
                    cooperative_callback_targets: Some(Vec::with_capacity(elements.len())),
                    callable_params_by_index: Some(Vec::with_capacity(elements.len())),
                    map_values_by_index: Some(Vec::with_capacity(elements.len())),
                    record_descendants_by_index: Some(Vec::with_capacity(elements.len())),
                    static_len: (!policy.storage_mutable).then_some(0),
                };
                for element in elements {
                    match element {
                        ArrayElement::Expr(expr) => {
                            if let Some(shapes) = observations.receiver_shapes_by_index.as_mut() {
                                shapes.push(receiver_shape_from_value(expr, self));
                            }
                            if let Some(metadata) =
                                observations.wrapped_value_metadata_by_index.as_mut()
                            {
                                metadata.push(self.wrapped_value_metadata_catalog_for_value(expr));
                            }
                            if let Some(targets) =
                                observations.cooperative_callback_targets.as_mut()
                            {
                                targets
                                    .push(self.cooperative_callback_target_for_value(expr, false));
                            }
                            let callable_params = if policy.storage_mutable
                                && self.namespace_member_value_metadata_origin_for_value(expr)
                            {
                                None
                            } else {
                                callable_param_info(expr, self)
                            };
                            if let Some(params) = observations.callable_params_by_index.as_mut() {
                                params.push(callable_params);
                            }
                            if let Some(map_values) = observations.map_values_by_index.as_mut() {
                                map_values.push(self.map_value_metadata_for_value(expr, false));
                            }
                            if let Some(record_descendants) =
                                observations.record_descendants_by_index.as_mut()
                            {
                                record_descendants
                                    .push(self.record_descendant_catalog_for_value(expr, false));
                            }
                            observations.static_len =
                                observations.static_len.and_then(|len| len.checked_add(1));
                        }
                        ArrayElement::Spread(spread) => {
                            let spread = self.spread_array_element_observations_for_value(spread);
                            observations.receiver_shapes_by_index = observations
                                .receiver_shapes_by_index
                                .and_then(|mut shapes| {
                                    shapes.extend(spread.receiver_shapes_by_index?);
                                    Some(shapes)
                                });
                            observations.wrapped_value_metadata_by_index = observations
                                .wrapped_value_metadata_by_index
                                .and_then(|mut metadata| {
                                    metadata.extend(spread.wrapped_value_metadata_by_index?);
                                    Some(metadata)
                                });
                            observations.cooperative_callback_targets = observations
                                .cooperative_callback_targets
                                .and_then(|mut targets| {
                                    targets.extend(spread.cooperative_callback_targets?);
                                    Some(targets)
                                });
                            observations.callable_params_by_index = observations
                                .callable_params_by_index
                                .and_then(|mut params| {
                                    params.extend(spread.callable_params_by_index?);
                                    Some(params)
                                });
                            observations.map_values_by_index =
                                observations.map_values_by_index.and_then(|mut map_values| {
                                    map_values.extend(spread.map_values_by_index?);
                                    Some(map_values)
                                });
                            observations.record_descendants_by_index = observations
                                .record_descendants_by_index
                                .and_then(|mut record_descendants| {
                                    record_descendants.extend(spread.record_descendants_by_index?);
                                    Some(record_descendants)
                                });
                            observations.static_len = observations
                                .static_len
                                .and_then(|len| len.checked_add(spread.static_len?));
                        }
                    }
                }
                observations
            }
            ExprKind::Ident => {
                let name = self.text(value.span);
                self.binding_lookup(name).map_or_else(
                    ArrayElementObservations::default,
                    |(_, info)| {
                        let local_storage = collection_storage_is_local(info);
                        let storage_metadata_available = local_storage
                            || !(info.mutable
                                || policy.storage_mutable && info.namespace_member_value_metadata);
                        let cooperative_metadata_available = local_storage
                            || !(info.mutable
                                || policy.cooperative_metadata_mutable
                                    && info.namespace_member_value_metadata);
                        ArrayElementObservations {
                            receiver_shapes_by_index: storage_metadata_available
                                .then(|| info.array_elements.receiver_shapes_by_index.clone()),
                            wrapped_value_metadata_by_index: storage_metadata_available.then(
                                || info.array_elements.wrapped_value_metadata_by_index.clone(),
                            ),
                            cooperative_callback_targets: cooperative_metadata_available
                                .then(|| info.array_elements.cooperative_callback_targets.clone()),
                            callable_params_by_index: storage_metadata_available
                                .then(|| info.array_elements.callable_params_by_index.clone()),
                            map_values_by_index: storage_metadata_available
                                .then(|| info.array_elements.map_values_by_index.clone()),
                            record_descendants_by_index: storage_metadata_available
                                .then(|| info.array_elements.record_descendants_by_index.clone()),
                            static_len: (local_storage || !policy.storage_mutable && !info.mutable)
                                .then_some(info.array_elements.static_len)
                                .flatten(),
                        }
                    },
                )
            }
            ExprKind::Member { .. } => self
                .namespace_value_metadata_for_member_expr(value)
                .map(|metadata| ArrayElementObservations {
                    receiver_shapes_by_index: Some(
                        metadata.array_elements.receiver_shapes_by_index.clone(),
                    ),
                    wrapped_value_metadata_by_index: Some(
                        metadata
                            .array_elements
                            .wrapped_value_metadata_by_index
                            .clone(),
                    ),
                    cooperative_callback_targets: (!policy.cooperative_metadata_mutable)
                        .then(|| metadata.array_elements.cooperative_callback_targets.clone()),
                    callable_params_by_index: Some(
                        metadata.array_elements.callable_params_by_index.clone(),
                    ),
                    map_values_by_index: Some(metadata.array_elements.map_values_by_index.clone()),
                    record_descendants_by_index: Some(
                        metadata.array_elements.record_descendants_by_index.clone(),
                    ),
                    static_len: (!policy.storage_mutable)
                        .then_some(metadata.array_elements.static_len)
                        .flatten(),
                })
                .unwrap_or_default(),
            ExprKind::Paren(inner) => self.array_element_observations_for_value(inner, policy),
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => {
                let Some(then_expr) = metadata_join_block_tail_expr(then_block) else {
                    return ArrayElementObservations::default();
                };
                let Some(else_expr) = else_branch.as_deref().and_then(metadata_join_else_expr)
                else {
                    return ArrayElementObservations::default();
                };
                join_array_element_observations([
                    self.array_element_observations_for_value(then_expr, policy),
                    self.array_element_observations_for_value(else_expr, policy),
                ])
            }
            ExprKind::Match { cases, .. }
                if !cases.is_empty()
                    && cases.last().is_some_and(match_case_is_unguarded_catch_all) =>
            {
                let observations = cases.iter().map(|case| {
                    metadata_join_match_body_expr(&case.body)
                        .map(|arm| self.array_element_observations_for_value(arm, policy))
                        .unwrap_or_default()
                });
                join_array_element_observations(observations)
            }
            _ => ArrayElementObservations::default(),
        }
    }
}
