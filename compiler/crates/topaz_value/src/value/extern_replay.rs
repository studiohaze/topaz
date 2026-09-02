use super::*;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ExternReplayKey {
    module: String,
    function: String,
    args_json: String,
}

/// Deterministic v5.4 extern replay sandbox table. Keys are the manifest module
/// name, exported function name, and canonical ABI encoding of the argument
/// vector.
///
/// This is the final v5.4 extern execution backend: manifest artifacts are
/// admitted and locked by the package layer, but runtime calls return only from
/// replay rows after the shared policy/budget checks below. Live artifact
/// execution is an experimental/post-v5.4 track, not part of this leaf.
#[derive(Debug, Clone, Default)]
pub struct ExternReplayStore {
    entries: Rc<BTreeMap<ExternReplayKey, Value>>,
    policies: Option<Rc<BTreeMap<String, ExternSandboxPolicy>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternSandboxKind {
    Replay,
    Wasm,
}

impl ExternSandboxKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ExternSandboxKind::Replay => "replay",
            ExternSandboxKind::Wasm => "wasm",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternSandboxPolicy {
    pub module: String,
    pub kind: ExternSandboxKind,
    pub artifact_path: Option<String>,
    pub fuel: Option<u64>,
    pub memory_bytes: Option<u64>,
}

impl ExternSandboxPolicy {
    pub fn new(
        module: impl Into<String>,
        kind: ExternSandboxKind,
        artifact_path: Option<String>,
        fuel: Option<u64>,
        memory_bytes: Option<u64>,
    ) -> Self {
        Self {
            module: module.into(),
            kind,
            artifact_path,
            fuel,
            memory_bytes,
        }
    }
}

impl ExternReplayStore {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn parse_jsonl(input: &str) -> Result<Self, String> {
        let mut entries = BTreeMap::new();
        Self::extend_jsonl_into(&mut entries, input)?;
        Ok(Self {
            entries: Rc::new(entries),
            policies: None,
        })
    }

    pub fn parse_jsonl_with_policies(
        input: &str,
        policies: Vec<ExternSandboxPolicy>,
    ) -> Result<Self, String> {
        let mut store = Self::parse_jsonl(input)?;
        store.set_policies(policies)?;
        Ok(store)
    }

    pub fn merge_jsonl(&mut self, input: &str) -> Result<(), String> {
        let mut entries = (*self.entries).clone();
        Self::extend_jsonl_into(&mut entries, input)?;
        self.entries = Rc::new(entries);
        Ok(())
    }

    pub fn set_policies(&mut self, policies: Vec<ExternSandboxPolicy>) -> Result<(), String> {
        let mut map = BTreeMap::new();
        for policy in policies {
            validate_extern_sandbox_policy(&policy)?;
            if map.insert(policy.module.clone(), policy).is_some() {
                return Err("extern sandbox policy declares a duplicate module".to_string());
            }
        }
        self.policies = Some(Rc::new(map));
        Ok(())
    }

    pub fn sandbox_policy(&self, module: &str) -> Option<&ExternSandboxPolicy> {
        self.policies.as_ref()?.get(module)
    }

    pub fn call(&self, module: &str, function: &str, args: &[Value]) -> Result<Value, String> {
        self.call_replay_sandbox(module, function, args)
    }

    pub fn call_replay_sandbox(
        &self,
        module: &str,
        function: &str,
        args: &[Value],
    ) -> Result<Value, String> {
        let policy = if let Some(policies) = &self.policies {
            let Some(policy) = policies.get(module) else {
                return Err(format!(
                    "extern sandbox policy for `{module}` is not available"
                ));
            };
            validate_extern_sandbox_policy(policy)?;
            Some(policy)
        } else {
            None
        };
        let args_json = canonical_abi_args_encode(args)?;
        let key = ExternReplayKey {
            module: module.to_string(),
            function: function.to_string(),
            args_json,
        };
        let result = self.entries.get(&key).cloned().ok_or_else(|| {
            format!(
                "extern replay has no row for `{module}.{function}` with canonical ABI args `{}`",
                key.args_json
            )
        })?;
        if let Some(policy) = policy {
            enforce_extern_replay_budget(policy, module, function, args, &key.args_json, &result)?;
        }
        Ok(result)
    }

    fn extend_jsonl_into(
        entries: &mut BTreeMap<ExternReplayKey, Value>,
        input: &str,
    ) -> Result<(), String> {
        for (line_idx, line) in input.lines().enumerate() {
            let line_no = line_idx + 1;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let node = json_parse(trimmed).map_err(|e| {
                format!(
                    "extern replay line {line_no}: invalid JSON at line {}, column {}: {}",
                    e.line, e.column, e.message
                )
            })?;
            let obj = abi_object(&node, &format!("line {line_no}"))?;
            abi_exact_fields(
                obj,
                &["args", "function", "module", "result"],
                &format!("line {line_no}"),
            )?;
            let module = abi_string_field(obj, "module", &format!("line {line_no}"))?;
            let function = abi_string_field(obj, "function", &format!("line {line_no}"))?;
            let JsonValue::Array(args) = abi_field(obj, "args", &format!("line {line_no}"))? else {
                return Err(format!(
                    "extern replay line {line_no}: `args` must be an array"
                ));
            };
            let args = args
                .iter()
                .enumerate()
                .map(|(i, arg)| decode_abi_value(arg, &format!("line {line_no}.args[{i}]"), 0))
                .collect::<Result<Vec<_>, _>>()?;
            let result = decode_abi_value(
                abi_field(obj, "result", &format!("line {line_no}"))?,
                &format!("line {line_no}.result"),
                0,
            )?;
            let key = ExternReplayKey {
                module: module.to_string(),
                function: function.to_string(),
                args_json: canonical_abi_args_encode(&args)?,
            };
            if entries.insert(key.clone(), result).is_some() {
                return Err(format!(
                    "extern replay line {line_no}: duplicate row for `{}.{}` with canonical ABI args `{}`",
                    key.module, key.function, key.args_json
                ));
            }
        }
        Ok(())
    }
}

pub(super) fn validate_extern_sandbox_policy(policy: &ExternSandboxPolicy) -> Result<(), String> {
    if policy.module.is_empty() {
        return Err("extern sandbox policy module must not be empty".to_string());
    }
    if policy.kind == ExternSandboxKind::Wasm && policy.artifact_path.is_none() {
        return Err(format!(
            "extern sandbox policy for `{}` kind `wasm` requires an artifact",
            policy.module
        ));
    }
    Ok(())
}

pub(super) fn enforce_extern_replay_budget(
    policy: &ExternSandboxPolicy,
    module: &str,
    function: &str,
    args: &[Value],
    args_json: &str,
    result: &Value,
) -> Result<(), String> {
    if let Some(limit) = policy.fuel {
        let used = extern_replay_fuel_used(args, result)?;
        if used > limit {
            return Err(format!(
                "extern replay fuel limit exceeded for `{module}.{function}`: used {used}, budget {limit}"
            ));
        }
    }
    if let Some(limit) = policy.memory_bytes {
        let result_json = canonical_abi_encode(result)?;
        let used = extern_replay_memory_bytes_used(args_json, &result_json)?;
        if used > limit {
            return Err(format!(
                "extern replay memory_bytes limit exceeded for `{module}.{function}`: used {used}, budget {limit}"
            ));
        }
    }
    Ok(())
}

pub(super) fn extern_replay_fuel_used(args: &[Value], result: &Value) -> Result<u64, String> {
    let mut used = 1_u64;
    for arg in args {
        used = abi_charge_add(used, abi_value_nodes(arg, 0)?)?;
    }
    abi_charge_add(used, abi_value_nodes(result, 0)?)
}

pub(super) fn extern_replay_memory_bytes_used(
    args_json: &str,
    result_json: &str,
) -> Result<u64, String> {
    let args_len = u64::try_from(args_json.len())
        .map_err(|_| "extern replay ABI args byte length exceeds u64".to_string())?;
    let result_len = u64::try_from(result_json.len())
        .map_err(|_| "extern replay ABI result byte length exceeds u64".to_string())?;
    abi_charge_add(args_len, result_len)
}

pub(super) fn abi_value_nodes(value: &Value, depth: u32) -> Result<u64, String> {
    if depth > JSON_MAX_DEPTH {
        return Err(
            "ABI_LIMIT: extern replay resource envelope exceeds the ABI value depth limit"
                .to_string(),
        );
    }
    let child_depth = depth + 1;
    let mut total = 1_u64;
    match value {
        Value::Some(inner) | Value::Ok(inner) | Value::Err(inner) => {
            total = abi_charge_add(total, abi_value_nodes(inner, child_depth)?)?;
        }
        Value::Array(items) => {
            let snapshot = items.borrow().clone();
            for item in &snapshot {
                total = abi_charge_add(total, abi_value_nodes(item, child_depth)?)?;
            }
        }
        Value::Record(fields) => {
            for field in fields.values() {
                total = abi_charge_add(total, abi_value_nodes(field, child_depth)?)?;
            }
        }
        Value::NominalRecord { fields, .. } => {
            for (_, field) in fields.iter() {
                total = abi_charge_add(total, abi_value_nodes(field, child_depth)?)?;
            }
        }
        Value::Enum { payloads, .. } => {
            for payload in payloads.iter() {
                total = abi_charge_add(total, abi_value_nodes(payload, child_depth)?)?;
            }
        }
        Value::Newtype { inner, .. } => {
            total = abi_charge_add(total, abi_value_nodes(inner, child_depth)?)?;
        }
        Value::Int(_)
        | Value::Bool(_)
        | Value::Str(_)
        | Value::Unit
        | Value::Null
        | Value::None
        | Value::Json(_)
        | Value::Bytes(_) => {}
        other => {
            return Err(format!(
                "ABI_UNSUPPORTED: extern replay resource envelope contains `{}`",
                other.kind()
            ));
        }
    }
    Ok(total)
}

pub(super) fn abi_charge_add(lhs: u64, rhs: u64) -> Result<u64, String> {
    lhs.checked_add(rhs)
        .ok_or_else(|| "extern replay resource envelope exceeds u64".to_string())
}

/// Shared extern-call leaf: both the interpreter and emitted programs cross the
/// same host boundary and map a missing/invalid replay binding to TPZ5032.
pub fn builtin_extern_call(
    host: &dyn Host,
    module: &str,
    function: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RtError> {
    host.extern_call(module, function, &args)
        .map_err(|e| fault(topaz_diag::extern_codes::REPLAY, e, span))
}

#[derive(Clone)]
pub struct ExternFunction {
    module: Rc<str>,
    function: Rc<str>,
    params: Rc<[Rc<str>]>,
    span: Span,
}

impl ExternFunction {
    pub fn new(module: &str, function: &str, params: &[&str], span: Span) -> Self {
        Self {
            module: Rc::from(module),
            function: Rc::from(function),
            params: params
                .iter()
                .map(|p| Rc::from(*p))
                .collect::<Vec<_>>()
                .into(),
            span,
        }
    }

    pub fn from_strings(module: String, function: String, params: Vec<String>, span: Span) -> Self {
        Self {
            module: Rc::from(module),
            function: Rc::from(function),
            params: params
                .into_iter()
                .map(|p| Rc::from(p.into_boxed_str()))
                .collect::<Vec<_>>()
                .into(),
            span,
        }
    }

    pub fn call_host(&self, host: &dyn Host, args: Vec<Value>) -> Result<Value, RtError> {
        builtin_extern_call(host, &self.module, &self.function, args, self.span)
    }

    pub fn arity(&self) -> usize {
        self.params.len()
    }

    pub fn param_name(&self, n: usize) -> Option<&str> {
        self.params.get(n).map(|p| p.as_ref())
    }
}

impl std::fmt::Debug for ExternFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<extern function>")
    }
}

impl TpzCall for ExternFunction {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Option<&str> {
        Some(&self.function)
    }

    fn call(&self, cx: RtCx, args: Vec<Value>) -> CallFuture {
        let this = self.clone();
        Box::pin(async move { this.call_host(&*cx.host(), args) })
    }

    fn arity(&self) -> usize {
        ExternFunction::arity(self)
    }

    fn param_name(&self, n: usize) -> Option<&str> {
        ExternFunction::param_name(self, n)
    }
}
