use crate::*;

pub(super) const ANONYMOUS_VARIADIC_TAIL_KW: &str = "__tpz_variadic_tail__";

pub type CheckedAliasSurfaces = BTreeMap<String, BTreeMap<String, topaz_check::ExportedAlias>>;
pub(super) type NamedValue = (String, Value);
pub(super) type ModuleTopBoundNameCounts = BTreeMap<String, usize>;
pub(super) struct SelectedDefaultImportBinding {
    pub(super) imported: String,
    pub(super) local: String,
}
pub(super) struct SelectedDefaultImport {
    pub(super) identity: String,
    pub(super) bindings: Vec<SelectedDefaultImportBinding>,
}
pub(super) struct NamespaceDefaultImportBinding {
    pub(super) identity: String,
    pub(super) local: Rc<str>,
}
#[derive(Default)]
pub(super) struct ModuleDefaultImportBindings {
    pub(super) selected: Vec<SelectedDefaultImport>,
    pub(super) namespaces: Vec<NamespaceDefaultImportBinding>,
    pub(super) namespace_by_local: BTreeMap<Rc<str>, usize>,
}
pub(super) type RecordDefaultConstCatalog = BTreeMap<String, Rc<[NamedValue]>>;
pub(super) type RecordDefaultSelfRuntimeValues = BTreeMap<String, Vec<(String, String)>>;
pub(super) type RecordDefaultRuntimeBindingCounts = BTreeMap<String, usize>;
pub(super) struct RecordDefaultStatementBindingFacts {
    pub(super) current: Rc<[String]>,
    pub(super) mutable: Rc<[String]>,
}
#[derive(Default)]
pub(super) struct RecordDefaultRuntimeBindingFacts {
    pub(super) counts: RecordDefaultRuntimeBindingCounts,
    pub(super) statements: Vec<RecordDefaultStatementBindingFacts>,
}
#[derive(Default)]
pub(super) struct ModuleBindingFacts {
    pub(super) top_bound_names: ModuleTopBoundNameCounts,
    pub(super) record_default_runtime_bindings: RecordDefaultRuntimeBindingFacts,
    pub(super) module_value_source_names: BTreeSet<String>,
}
pub(super) type NamespaceRuntimeDefaultCandidates = BTreeMap<String, Vec<(String, String)>>;
#[derive(Default)]
pub(super) struct RuntimeDefaultNameFacts {
    pub(super) immutable_lets: BTreeSet<String>,
    pub(super) exported_values: BTreeSet<String>,
}
#[derive(Default)]
pub(super) struct ModuleDefaultConstFacts {
    pub(super) own: Rc<[NamedValue]>,
    pub(super) exported: Rc<[NamedValue]>,
}
#[derive(Default)]
pub(super) struct ModuleDefaultInputFacts {
    pub(super) imports: ModuleDefaultImportBindings,
    pub(super) runtime_names: RuntimeDefaultNameFacts,
    pub(super) const_values: ModuleDefaultConstFacts,
    pub(super) self_runtime_values: Rc<RecordDefaultSelfRuntimeValues>,
    pub(super) record_default_runtime_bindings: Rc<RecordDefaultRuntimeBindingFacts>,
    pub(super) module_value_source_names: Rc<[String]>,
    pub(super) module_top_bound_names: Rc<BTreeSet<String>>,
    pub(super) namespace_runtime_candidates: Rc<NamespaceRuntimeDefaultCandidates>,
}
pub(super) type ModuleDefaultInputCatalog = BTreeMap<String, Rc<ModuleDefaultInputFacts>>;
pub(super) type SpreadFaultParts = (Vec<String>, Vec<String>, Vec<String>);
pub(super) type ReceiverMutatingSpreadRenderer = fn(&str, &[String], Span) -> String;
pub(super) type ReceiverMutatingSpreadSpec =
    (&'static [&'static str], ReceiverMutatingSpreadRenderer);

#[derive(Debug, Clone, Copy)]
pub(super) enum PythonRunMode<'a> {
    Trace,
    Application {
        fs_read_roots: &'a [String],
        fs_write_roots: &'a [String],
    },
}

pub(super) fn positional_args(args: &[CallArg]) -> Result<Vec<&Expr>, PyEmitError> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        match arg {
            CallArg::Positional(expr) => out.push(expr),
            CallArg::Spread(_) | CallArg::Named { .. } => {
                return Err(PyEmitError::unsupported("call argument shape").at(call_arg_span(arg)));
            }
        }
    }
    Ok(out)
}

pub(super) fn call_arg_span(arg: &CallArg) -> Span {
    match arg {
        CallArg::Positional(expr) | CallArg::Spread(expr) => expr.span,
        CallArg::Named { name, .. } => name.span,
    }
}

pub(super) fn binding_name<'a>(
    pattern: &Pattern,
    map: &'a SourceMap,
) -> Result<&'a str, PyEmitError> {
    match &pattern.kind {
        PatternKind::Binding(name) | PatternKind::Typed { name, .. } => {
            Ok(text_in_map(map, name.span))
        }
        _ => Err(PyEmitError::unsupported("binding pattern").at(pattern.span)),
    }
}

pub(super) fn pattern_type(pattern: &Pattern) -> Option<&Type> {
    match &pattern.kind {
        PatternKind::Typed { ty, .. } => Some(ty),
        _ => None,
    }
}

pub(super) fn decode_string_parts(
    parts: &[StringPart],
    map: &SourceMap,
) -> Result<String, PyEmitError> {
    let mut decoded = String::new();
    for part in parts {
        match part {
            StringPart::Text(span) => {
                decode_escapes(text_in_map(map, *span), &mut decoded, *span)
                    .map_err(|_| PyEmitError::malformed_literal("string escape"))?;
            }
            StringPart::Interpolation(expr) => {
                return Err(
                    PyEmitError::unsupported("interpolation in constant decoder").at(expr.span),
                );
            }
        }
    }
    Ok(decoded)
}

pub(super) fn text(src: &str, span: Span) -> &str {
    &src[span.lo as usize..span.hi as usize]
}

pub(super) fn text_in_map(map: &SourceMap, span: Span) -> &str {
    text(map.file(span.file).src(), span)
}

pub(super) fn py_span(span: Span) -> String {
    format!("({}, {}, {})", span.file.0, span.lo, span.hi)
}

pub(super) fn py_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => write!(out, "\\u{:04x}", c as u32).expect("write to string"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(super) fn py_string_list(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| py_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub(super) fn extern_sandbox_policies_json(policies: &[ExternSandboxPolicy]) -> String {
    if policies.is_empty() {
        return String::new();
    }
    let mut out = String::from("[");
    for (idx, policy) in policies.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("{\"artifact_path\":");
        match &policy.artifact_path {
            Some(path) => out.push_str(&py_string(path)),
            None => out.push_str("null"),
        }
        out.push_str(",\"fuel\":");
        match policy.fuel {
            Some(fuel) => write!(out, "{fuel}").expect("write to string"),
            None => out.push_str("null"),
        }
        out.push_str(",\"kind\":");
        out.push_str(&py_string(policy.kind.as_str()));
        out.push_str(",\"memory_bytes\":");
        match policy.memory_bytes {
            Some(memory_bytes) => write!(out, "{memory_bytes}").expect("write to string"),
            None => out.push_str("null"),
        }
        out.push_str(",\"module\":");
        out.push_str(&py_string(&policy.module));
        out.push('}');
    }
    out.push(']');
    out
}

pub(super) fn write_global_assignment(
    out: &mut String,
    indent: usize,
    py_name: &str,
    value_py: &str,
    source_name: &str,
) {
    writeln!(
        out,
        "{}globals()[{}] = {value_py}  # {}",
        " ".repeat(indent),
        py_string(py_name),
        py_comment_name(source_name)
    )
    .expect("write to string");
}

pub(super) fn self_runtime_default_py_source_names(
    runtime_values: &BTreeMap<String, Vec<(String, String)>>,
    external_runtime_values: Option<&[(String, String)]>,
) -> BTreeMap<String, String> {
    let mut source_names = BTreeMap::new();
    for_each_self_runtime_default_ref(
        runtime_values,
        external_runtime_values,
        |source_name, py_name| {
            source_names.insert(source_name.to_string(), py_name.to_string());
        },
    );
    source_names
}

pub(super) fn write_self_runtime_default_py_seeds(
    out: &mut String,
    indent: usize,
    runtime_values: &BTreeMap<String, Vec<(String, String)>>,
    external_runtime_values: Option<&[(String, String)]>,
) {
    let mut py_names = BTreeSet::new();
    for_each_self_runtime_default_ref(runtime_values, external_runtime_values, |_, py_name| {
        py_names.insert(py_name);
    });
    for py_name in py_names {
        writeln!(
            out,
            "{}globals()[{}] = __tpz_missing",
            " ".repeat(indent),
            py_string(py_name)
        )
        .expect("write to string");
    }
}

pub(super) fn for_each_self_runtime_default_ref<'a>(
    runtime_values: &'a BTreeMap<String, Vec<(String, String)>>,
    external_runtime_values: Option<&'a [(String, String)]>,
    mut visit: impl FnMut(&'a str, &'a str),
) {
    const EXTERNAL_KEY: &str = "__topaz_external";
    let mut external_runtime_values = external_runtime_values.filter(|refs| !refs.is_empty());
    let visit_external = |refs: &'a [(String, String)], visit: &mut dyn FnMut(&'a str, &'a str)| {
        for (source_name, py_name) in refs {
            visit(source_name, py_name);
        }
    };
    for (key, refs) in runtime_values {
        if key.as_str() > EXTERNAL_KEY
            && let Some(refs) = external_runtime_values.take()
        {
            visit_external(refs, &mut visit);
        }
        for (source_name, py_name) in refs {
            visit(source_name, py_name);
        }
        if key == EXTERNAL_KEY
            && let Some(refs) = external_runtime_values.take()
        {
            visit_external(refs, &mut visit);
        }
    }
    if let Some(refs) = external_runtime_values {
        visit_external(refs, &mut visit);
    }
}

pub(super) fn emit_defer_helpers(out: &mut String, indent: usize) {
    let pad = " ".repeat(indent);
    let inner_pad = " ".repeat(indent + 4);
    let body_pad = " ".repeat(indent + 8);
    writeln!(out, "{pad}def __tpz_run_defers_to(__tpz_mark):").expect("write to string");
    writeln!(out, "{inner_pad}while len(__tpz_defers) > __tpz_mark:").expect("write to string");
    writeln!(out, "{body_pad}tpz_run_defer(host, __tpz_defers.pop())").expect("write to string");
    writeln!(out, "{pad}def __tpz_run_defers():").expect("write to string");
    writeln!(out, "{inner_pad}__tpz_run_defers_to(0)").expect("write to string");
}

pub(super) fn py_tuple(items: Vec<String>) -> String {
    match items.len() {
        0 => "()".to_string(),
        1 => format!("({},)", items[0]),
        _ => format!("({})", items.join(", ")),
    }
}

pub(super) fn py_comment_name(name: &str) -> String {
    name.chars()
        .map(|ch| match ch {
            '\n' | '\r' => ' ',
            other => other,
        })
        .collect()
}

pub(super) fn mangle(name: &str) -> String {
    let mut out = String::from("_t_");
    for byte in name.bytes() {
        write!(out, "{byte:02x}").expect("write to string");
    }
    out
}

pub(super) fn cooperative_function_py_name(py_name: &str) -> String {
    format!("{py_name}__co")
}

pub(super) fn callable_param_shapes_match(
    lhs: &[FunctionParamInfo],
    rhs: &[FunctionParamInfo],
) -> bool {
    lhs.len() == rhs.len()
        && lhs.iter().zip(rhs).all(|(lhs, rhs)| {
            lhs.source_name == rhs.source_name
                && lhs.has_default == rhs.has_default
                && lhs.variadic == rhs.variadic
        })
}

pub(super) fn homogeneous_array_callable_params(
    slots: &[Option<Vec<FunctionParamInfo>>],
) -> Option<Vec<FunctionParamInfo>> {
    let mut slots = slots.iter();
    let first = slots.next()?.as_ref()?;
    for slot in slots {
        let params = slot.as_ref()?;
        if !callable_param_shapes_match(first, params) {
            return None;
        }
    }
    Some(first.clone())
}

pub(super) fn binding_info_has_value_metadata(info: &BindingInfo) -> bool {
    info.callable_params.is_some()
        || info.cooperative_callback_py_name.is_some()
        || !info.array_elements.is_empty()
        || !info.declared_record_descendants.is_empty()
        || !info.record_descendants.is_empty()
        || !info.map_value.is_empty()
        || binding_receiver_shape(info).is_some()
        || !info.wrapped_value_metadata.is_empty()
}

pub(super) fn binding_receiver_shape(info: &BindingInfo) -> Option<ReceiverShape> {
    [
        (info.string, ReceiverShape::String),
        (info.template, ReceiverShape::Template),
        (info.array, ReceiverShape::Array),
        (info.map, ReceiverShape::Map),
        (info.bytes, ReceiverShape::Bytes),
        (info.byte_buffer, ReceiverShape::ByteBuffer),
        (info.json, ReceiverShape::Json),
        (info.option, ReceiverShape::Option),
        (info.result, ReceiverShape::Result),
    ]
    .into_iter()
    .find_map(|(matches, shape)| matches.then_some(shape))
}

pub(super) fn set_namespace_member_value_metadata_from_origin(
    info: &mut BindingInfo,
    origin: bool,
) {
    info.namespace_member_value_metadata = origin && binding_info_has_value_metadata(info);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CollectionStorageIdentity {
    Namespace(String),
    Local { module: String, span: Span },
}

impl CollectionStorageIdentity {
    pub(super) fn is_local(&self) -> bool {
        matches!(self, Self::Local { .. })
    }
}

pub(super) fn collection_storage_has_namespace_metadata_origin(info: &BindingInfo) -> bool {
    matches!(
        info.collection_storage_identity,
        Some(CollectionStorageIdentity::Namespace(_))
    )
}

pub(super) fn collection_storage_is_local(info: &BindingInfo) -> bool {
    info.collection_storage_identity
        .as_ref()
        .is_some_and(CollectionStorageIdentity::is_local)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct BindingInfo {
    pub(super) py_name: Option<String>,
    pub(super) forward_function_cell: bool,
    pub(super) cooperative_py_name: Option<String>,
    pub(super) cooperative_callback_py_name: Option<String>,
    pub(super) cooperative_callback_needs_host: bool,
    pub(super) array_elements: ArrayElementMetadata,
    pub(super) declared_record_descendants: RecordDescendantCatalog,
    pub(super) record_descendants: RecordDescendantCatalog,
    pub(super) map_value: MapValueMetadata,
    pub(super) namespace_member_value_metadata: bool,
    pub(super) collection_storage_identity: Option<CollectionStorageIdentity>,
    pub(super) mutable: bool,
    pub(super) namespace_import: bool,
    pub(super) typed_rebind_callable_params: Option<Vec<FunctionParamInfo>>,
    pub(super) callable_params: Option<Vec<FunctionParamInfo>>,
    pub(super) mutated_collection_params: BTreeSet<usize>,
    pub(super) callable_params_flow_allowed: bool,
    pub(super) composed: bool,
    pub(super) string: bool,
    pub(super) template: bool,
    pub(super) array: bool,
    pub(super) map: bool,
    pub(super) bytes: bool,
    pub(super) byte_buffer: bool,
    pub(super) json: bool,
    pub(super) option: bool,
    pub(super) result: bool,
    pub(super) wrapped_value_metadata: WrappedValueMetadataCatalog,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum LoopFrameKind {
    Plain,
    Value,
}
